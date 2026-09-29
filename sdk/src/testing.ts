// Runs a plugin in `bun test` the way the app would, without the app: it
// renders the surfaces, clicks and types through the same messages, and
// records what the plugin asks for.
//
//   import { mount } from "@sidedoor/sdk/testing";
//   import pomodoro from "./index";
//
//   test("starts", async () => {
//     const plugin = mount(pomodoro);
//     await plugin.press("Start");
//     expect(plugin.text()).toContain("Focusing");
//   });

import { start, type HostMessage, type PluginDefinition, type PluginMessage } from "./host";
import { invalidate, reset, type Patch } from "./runtime";
import { createStorage, type Storage } from "./storage";
import type { Node } from "./types";

export interface MountOptions {
  /** Saved settings; defaults fill the gaps. */
  settings?: Record<string, unknown>;
  /** What `useStorage` starts with. Nothing is written to disk. */
  storage?: Record<string, unknown>;
}

/** An element in a rendered surface. */
export interface Found {
  type: string;
  props: Record<string, unknown>;
  children: Node[];
  /** All the text inside it. */
  text(): string;
  /** Calls its `on_click`. */
  click(): Promise<void>;
  /** Calls `on_change`, as typing, switching or picking would. */
  change(value: unknown): Promise<void>;
  /** Calls `on_submit`, as Return in an `Input` would. */
  submit(value: string): Promise<void>;
}

/** `"card"`, `"tile"` or `"window:<key>"`. */
type Surface = "card" | "tile" | `window:${string}`;

/** Lets the plugin re-render and run its effects. */
const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

function textOf(nodes: Node[]): string {
  return nodes
    .map((node) => (typeof node === "string" ? node : textOf(node.c)))
    .join("");
}

function nodeAt(tree: Node[], path: number[]): { parent: Node[]; index: number } | null {
  let parent = tree;
  for (const index of path.slice(0, -1)) {
    const node = parent[index];
    if (node === undefined || typeof node === "string") return null;
    parent = node.c;
  }
  const index = path[path.length - 1];
  return index !== undefined && index < parent.length ? { parent, index } : null;
}

function applyPatch(tree: Node[], patch: Patch) {
  const at = nodeAt(tree, patch.path);
  if (!at) throw new Error(`A patch doesn't fit the tree: ${JSON.stringify(patch)}`);
  if (patch.op === "replace") {
    at.parent[at.index] = patch.node;
  } else {
    const node = at.parent[at.index];
    if (typeof node === "string") throw new Error("A props patch points at text.");
    at.parent[at.index] = { ...node, p: patch.props };
  }
}

/** The label a person would read on an element: its `label` or `title`, else its text. */
function labelOf(node: Exclude<Node, string>): string {
  const { label, title } = node.p;
  if (typeof label === "string") return label;
  if (typeof title === "string") return title;
  return textOf(node.c).trim();
}

export function mount(definition: PluginDefinition<any>, options: MountOptions = {}) {
  reset();
  const sent: PluginMessage[] = [];
  const trees: Partial<Record<Surface, Node[]>> = {};
  let receive: (message: HostMessage) => void = () => {};

  const storage: Storage = createStorage(null, invalidate);
  for (const [key, value] of Object.entries(options.storage ?? {})) storage.set(key, value);

  start(
    definition,
    {
      send(message) {
        sent.push(message);
        // Answer as the app would when a window opens or closes.
        if (message.type === "open_window" || message.type === "close_window") {
          const open = message.type === "open_window";
          if (!open) delete trees[`window:${message.key}`];
          queueMicrotask(() => receive({ type: "window", key: message.key, open }));
        }
        if (message.type === "render") {
          trees[message.surface as Surface] = structuredClone(message.tree);
        } else if (message.type === "patch") {
          const tree = trees[message.surface as Surface];
          if (!tree) throw new Error(`A patch for ${message.surface} before it rendered.`);
          for (const patch of message.patches) applyPatch(tree, patch);
        }
      },
      listen(handler) {
        receive = handler;
      },
    },
    { settings: options.settings ?? {}, storage },
  );

  const deliver = async (message: HostMessage) => {
    receive(message);
    await settle();
    const error = sent.find((message) => message.type === "error");
    if (error) throw new Error(`The plugin failed: ${error.message}`);
  };

  const handler = (node: Exclude<Node, string>, prop: string) => {
    const reference = node.p[prop] as { $h?: string } | undefined;
    if (!reference?.$h) {
      throw new Error(`<${node.t}> "${labelOf(node)}" has no ${prop}.`);
    }
    return reference.$h;
  };

  const found = (node: Exclude<Node, string>): Found => ({
    type: node.t,
    props: node.p,
    children: node.c,
    text: () => textOf(node.c),
    click: () => deliver({ type: "event", handler: handler(node, "on_click") }),
    change: (value) => deliver({ type: "event", handler: handler(node, "on_change"), value }),
    submit: (value) => deliver({ type: "event", handler: handler(node, "on_submit"), value }),
  });

  const all = (surface: Surface): Array<Exclude<Node, string>> => {
    const out: Array<Exclude<Node, string>> = [];
    const visit = (nodes: Node[]) => {
      for (const node of nodes) {
        if (typeof node === "string") continue;
        out.push(node);
        visit(node.c);
      }
    };
    visit(trees[surface] ?? []);
    return out;
  };

  const plugin = {
    /** The card as the app has it now. */
    get card(): Node[] {
      return trees.card ?? [];
    },
    /** The dock tile, if the plugin draws one. */
    get tile(): Node[] | undefined {
      return trees.tile;
    },
    /** What an open window shows, by its key; `undefined` while closed. */
    window: (key: string): Node[] | undefined => trees[`window:${key}`],
    /** Closes a window as its close button would. */
    closeWindow: async (key: string) => {
      delete trees[`window:${key}`];
      await deliver({ type: "window", key, open: false });
    },
    /** Everything the plugin sent, oldest first. */
    sent,
    /** `sidedoor.notify` calls, as `{ title, body }`. */
    get notifications() {
      return sent.flatMap((message) =>
        message.type === "notify" ? [{ title: message.title, body: message.body }] : [],
      );
    },
    /** What `useStorage` saved. */
    storage,

    /** All the text on a surface. */
    text: (surface: Surface = "card") => textOf(trees[surface] ?? []),

    /**
     * The first element whose label (a `label` or `title` prop, else its
     * text) is `label`, or that `match` accepts.
     */
    find(label: string | ((element: Found) => boolean), surface: Surface = "card"): Found {
      const match = all(surface)
        .map(found)
        .find((element, index) =>
          typeof label === "string"
            ? labelOf(all(surface)[index] as Exclude<Node, string>) === label
            : label(element),
        );
      if (!match) throw new Error(`Nothing labelled ${JSON.stringify(label)} on the ${surface}.`);
      return match;
    },
    /** Every element of `type`, e.g. `"Button"`. */
    findAll: (type: string, surface: Surface = "card") =>
      all(surface)
        .filter((node) => node.t === type)
        .map(found),

    /** Clicks the element labelled `label` that has an `on_click`. */
    press(label: string, surface: Surface = "card") {
      const target = all(surface).find((node) => labelOf(node) === label && node.p.on_click);
      if (!target) throw new Error(`Nothing clickable labelled ${JSON.stringify(label)}.`);
      return found(target).click();
    },
    /** Clicks the dock tile. */
    click: () => deliver({ type: "click" }),
    /** Runs a context-menu command by its key. */
    action: (key: string) => deliver({ type: "action", key }),
    /** Opens or closes the card. */
    setCardOpen: (open: boolean) => deliver({ type: "card", open }),
    /** Changes settings as Settings › Plugins would. */
    setSettings: (values: Record<string, unknown>) => deliver({ type: "settings", values }),
    /** Waits for re-renders and effects, e.g. after a timer fires. */
    settle,
    /** Stops timers and effects. */
    unmount: reset,
  };
  return plugin;
}
