// The bridge to the app. Inside the app every plugin runs in its own Worker
// thread of one Bun process (see `supervisor.ts`), and talks to it with
// messages: rendered surfaces and patches out, events and settings in.
// `@sidedoor/sdk/testing` runs the same bridge without a worker.

import {
  diff,
  dispatch,
  invalidate,
  renderSurfaces,
  restore,
  runEffects,
  setInvalidateHandler,
  snapshot,
  type Patch,
  type Snapshot,
} from "./runtime";
import { createStorage, type Storage } from "./storage";
import type { Child, Component, IconName, Node } from "./types";

/** Messages the app sends a plugin. */
export type HostMessage =
  | { type: "event"; handler: string; value?: unknown }
  | { type: "card"; open: boolean }
  /** The dock tile was clicked, or the item's shortcut pressed. */
  | { type: "click" }
  /** A command from the item's context menu. */
  | { type: "action"; key: string }
  | { type: "settings"; values: Record<string, unknown> }
  /** The app lost track of a surface; send everything again. */
  | { type: "resync" }
  /** From the supervisor before a reload: reply with the state to keep. */
  | { type: "snapshot" };

/** Messages a plugin sends the app. */
export type PluginMessage =
  | ({ type: "manifest" } & Manifest)
  | { type: "render"; surface: string; tree: Node[] }
  | { type: "patch"; surface: string; patches: Patch[] }
  | { type: "open_url"; url: string }
  | { type: "open_path"; path: string }
  | { type: "copy"; text: string }
  | { type: "notify"; title: string; body: string }
  | { type: "log"; line: string }
  | { type: "error"; message: string }
  /** To the supervisor only, in reply to `snapshot`. */
  | { type: "snapshot"; state: Snapshot };

// The worker's globals, typed here so plugins typecheck without `@types/bun`.
interface WorkerScope {
  process: { env: Record<string, string | undefined> };
  Bun: { inspect(value: unknown): string };
  postMessage(message: unknown): void;
  addEventListener(type: "message", listener: (event: { data: HostMessage }) => void): void;
}
const scope = globalThis as unknown as WorkerScope;
const env = scope.process?.env ?? {};

/** How a running plugin reaches the app: a worker's messages, or a test. */
export interface Bridge {
  send(message: PluginMessage): void;
  listen(receive: (message: HostMessage) => void): void;
}

/** What the plugin running in this thread knows about the app. */
interface Session {
  bridge: Bridge;
  cardOpen: boolean;
  /** What the user saved; defaults fill the gaps. */
  saved: Record<string, unknown>;
  defaults: Record<string, unknown>;
  storage: Storage;
}

function parseJson<T>(json: string | undefined, fallback: T): T {
  try {
    return json ? JSON.parse(json) : fallback;
  } catch {
    return fallback;
  }
}

const workerBridge: Bridge = {
  send: (message) => scope.postMessage(message),
  listen: (receive) => scope.addEventListener("message", ({ data }) => receive(data)),
};

/**
 * Plugin code at module level (a store's first value, say) runs before
 * `definePlugin`, so the session starts with what the worker was given.
 */
let session: Session = {
  bridge: workerBridge,
  cardOpen: false,
  saved: parseJson(env.SIDEDOOR_SETTINGS, {}),
  defaults: {},
  storage: createStorage(
    env.SIDEDOOR_DATA_DIR ? `${env.SIDEDOOR_DATA_DIR}/storage.json` : null,
    invalidate,
  ),
};

// State from before a hot reload, for the stores and components made next.
restore(parseJson<Snapshot | null>(env.SIDEDOOR_SNAPSHOT, null));

function send(message: PluginMessage) {
  session.bridge.send(message);
}

function describeError(error: unknown) {
  return error instanceof Error ? (error.stack ?? error.message) : String(error);
}

function settingValues(): Record<string, unknown> {
  return { ...session.defaults, ...session.saved };
}

/** Whether the widget's card is showing. Read it during render. */
export function useCardOpen(): boolean {
  return session.cardOpen;
}

/**
 * A value from the plugin's settings, as declared in `definePlugin` and
 * edited in Settings › Plugins. Surfaces also get them, typed, as
 * `settings`.
 */
export function useSetting<T = unknown>(key: string): T {
  return settingValues()[key] as T;
}

/**
 * A value saved in the plugin's data folder, which survives reloads and
 * restarts. Like `useState`, but every component that reads `key` shares
 * it. Values must be JSON.
 */
export function useStorage<T>(
  key: string,
  initial: T,
): [T, (value: T | ((previous: T) => T)) => void] {
  const { storage } = session;
  const value = storage.has(key) ? (storage.get(key) as T) : initial;
  const set = (next: T | ((previous: T) => T)) => {
    const previous = storage.has(key) ? (storage.get(key) as T) : initial;
    storage.set(key, typeof next === "function" ? (next as (previous: T) => T)(previous) : next);
  };
  return [value, set];
}

/** Things a widget can ask the app to do. */
export const sidedoor = {
  openUrl: (url: string) => send({ type: "open_url", url }),
  open: (path: string) => send({ type: "open_path", path }),
  copy: (text: string) => send({ type: "copy", text }),
  /** Shows a banner in Notification Center, under the plugin's name. */
  notify: ({ title, body = "" }: { title: string; body?: string }) =>
    send({ type: "notify", title, body }),
  /** What `useStorage` saves, for use outside render. */
  get storage(): Storage {
    return session.storage;
  },
  /** A folder the plugin can keep files in. */
  dataDir: env.SIDEDOOR_DATA_DIR ?? "",
  /** The plugin's settings right now. */
  settings: (): Record<string, unknown> => settingValues(),
};

/** A setting the user can change in Settings › Plugins. */
export type SettingDefinition = {
  title: string;
  description?: string;
} & (
  | { type: "text" | "secret"; default?: string }
  | { type: "number"; default?: number }
  | { type: "toggle"; default?: boolean }
  | { type: "choice"; options: readonly string[]; default?: string }
);

export type SettingDefinitions = Record<string, SettingDefinition>;

/** The value a setting holds: a choice is one of its options. */
export type SettingValue<D extends SettingDefinition> = D extends { type: "number" }
  ? number
  : D extends { type: "toggle" }
    ? boolean
    : D extends { type: "choice"; options: readonly (infer Option)[] }
      ? Option
      : string;

export type SettingValues<S extends SettingDefinitions> = {
  [Key in keyof S]: SettingValue<S[Key]>;
};

/** What surfaces, `onClick` and actions are given. */
export interface PluginContext<S extends SettingDefinitions = SettingDefinitions> {
  /** Every setting, typed from `definePlugin`; surfaces re-render when they change. */
  settings: SettingValues<S>;
}

export type Surface<S extends SettingDefinitions = SettingDefinitions> = (
  context: PluginContext<S>,
) => Child;

/** A command in the dock item's context menu. */
export interface PluginAction<S extends SettingDefinitions = SettingDefinitions> {
  title: string;
  run: (context: PluginContext<S>) => void;
}

export interface PluginDefinition<S extends SettingDefinitions = SettingDefinitions> {
  /** Shown in the dock's menus and in Settings. */
  name: string;
  /** A Lucide icon name, e.g. `"timer"`; the dock shows it until `tile` draws. */
  icon?: IconName;
  /** Card width in points; 280 by default. */
  width?: number;
  /** Card height in points. Leave it out and the card fits its content. */
  height?: number;
  /** Settings by key; surfaces read them as `settings`. */
  settings?: S;
  /** Drawn in the dock slot, about 44 points square. */
  tile?: Surface<S>;
  /** Drawn in the card that opens on hover. */
  card: Surface<S>;
  /**
   * Runs when the dock tile is clicked. With it, the item's global shortcut
   * clicks too, instead of showing the card.
   */
  onClick?: (context: PluginContext<S>) => void;
  /** Commands for the item's context menu, by key. */
  actions?: Record<string, PluginAction<S>>;
}

/** What the app learns about a plugin when it starts. */
export interface Manifest {
  name: string;
  icon: string;
  width: number;
  height: number | null;
  settings: Array<{ key: string } & SettingDefinition>;
  clickable: boolean;
  actions: Array<{ key: string; title: string }>;
}

function defaultOf(setting: SettingDefinition): unknown {
  if (setting.default !== undefined) return setting.default;
  switch (setting.type) {
    case "number":
      return 0;
    case "toggle":
      return false;
    case "choice":
      return setting.options[0] ?? null;
    default:
      return "";
  }
}

/** The manifest and default settings a definition describes. */
export function describe<S extends SettingDefinitions>(
  definition: PluginDefinition<S>,
): {
  manifest: Manifest;
  defaults: Record<string, unknown>;
} {
  const settings: Array<[string, SettingDefinition]> = Object.entries(definition.settings ?? {});
  return {
    manifest: {
      name: definition.name,
      icon: definition.icon ?? "puzzle",
      width: definition.width ?? 280,
      height: definition.height ?? null,
      settings: settings.map(([key, setting]) => ({ key, ...setting })),
      clickable: definition.onClick !== undefined,
      actions: Object.entries(definition.actions ?? {}).map(([key, action]) => ({
        key,
        title: action.title,
      })),
    },
    defaults: Object.fromEntries(settings.map(([key, setting]) => [key, defaultOf(setting)])),
  };
}

/**
 * Declares the plugin: what it's called, how it looks in the dock and its
 * settings, all in one place. Inside the app it also starts the plugin.
 */
export function definePlugin<const S extends SettingDefinitions = {}>(
  definition: PluginDefinition<S>,
): PluginDefinition<S> {
  if (env.SIDEDOOR_PLUGIN === "1") start(definition as PluginDefinition, workerBridge);
  return definition;
}

/** Options for `start` outside a worker. */
export interface StartOptions {
  settings?: Record<string, unknown>;
  storage?: Storage;
}

/**
 * Runs a plugin against `bridge`. Inside the app, `definePlugin` does this;
 * tests use `mount` from `@sidedoor/sdk/testing`.
 */
export function start(definition: PluginDefinition, bridge: Bridge, options: StartOptions = {}) {
  const { manifest, defaults } = describe(definition);
  session = {
    ...session,
    bridge,
    cardOpen: false,
    defaults,
    saved: options.settings ?? session.saved,
    storage: options.storage ?? session.storage,
  };

  if (bridge === workerBridge) {
    // Logs go to the app, which shows them in Settings › Plugins.
    const log = (...args: unknown[]) =>
      send({
        type: "log",
        line: args.map((arg) => (typeof arg === "string" ? arg : scope.Bun.inspect(arg))).join(" "),
      });
    console.log = log;
    console.info = log;
    console.debug = log;
    console.warn = log;
    console.error = log;
  }

  send({ type: "manifest", ...manifest });

  const surfaces: Record<string, Component<any>> = { card: definition.card };
  if (definition.tile) surfaces.tile = definition.tile;
  const context: PluginContext = {
    get settings() {
      return settingValues() as PluginContext["settings"];
    },
  };
  let sent = new Map<string, Node[]>();

  const render = () => {
    try {
      const output = renderSurfaces(surfaces, context as unknown as Record<string, unknown>);
      for (const [surface, tree] of Object.entries(output)) {
        const previous = sent.get(surface);
        sent.set(surface, tree);
        const patches = previous ? diff(previous, tree) : null;
        if (patches === null) {
          send({ type: "render", surface, tree });
        } else if (patches.length > 0) {
          send({ type: "patch", surface, patches });
        }
      }
      runEffects();
    } catch (error) {
      send({ type: "error", message: describeError(error) });
    }
  };
  setInvalidateHandler(render);
  render();

  const guarded = (run: () => void) => {
    try {
      run();
    } catch (error) {
      send({ type: "error", message: describeError(error) });
    }
  };

  bridge.listen((message) => {
    switch (message.type) {
      case "event":
        guarded(() => dispatch(message.handler, message.value));
        break;
      case "click":
        if (definition.onClick) guarded(() => definition.onClick?.(context));
        break;
      case "action": {
        const action = definition.actions?.[message.key];
        if (action) guarded(() => action.run(context));
        break;
      }
      case "card":
        session.cardOpen = message.open;
        break;
      case "settings":
        session.saved = message.values;
        break;
      case "resync":
        sent = new Map();
        break;
      case "snapshot":
        send({ type: "snapshot", state: snapshot() });
        return;
    }
    // Handlers usually change state; re-render in case they changed
    // something outside a hook. Unchanged surfaces send nothing.
    invalidate();
  });
}
