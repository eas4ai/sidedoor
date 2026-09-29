// The bridge to the app. Inside the app every plugin runs in its own Worker
// thread of one Bun process (see `supervisor.ts`), and talks to it with
// messages: rendered surfaces and patches out, events and settings in.

import {
  diff,
  dispatch,
  invalidate,
  renderSurfaces,
  runEffects,
  setInvalidateHandler,
  type Patch,
} from "./runtime";
import type { Component, Node } from "./types";

/** Messages the app sends a plugin. */
export type HostMessage =
  | { type: "event"; handler: string; value?: unknown }
  | { type: "card"; open: boolean }
  | { type: "settings"; values: Record<string, unknown> }
  /** The app lost track of a surface; send everything again. */
  | { type: "resync" };

/** Messages a plugin sends the app. */
export type PluginMessage =
  | { type: "render"; surface: string; tree: Node[] }
  | { type: "patch"; surface: string; patches: Patch[] }
  | { type: "open_url"; url: string }
  | { type: "open_path"; path: string }
  | { type: "copy"; text: string }
  | { type: "log"; line: string }
  | { type: "error"; message: string };

// The worker's globals, typed here so plugins typecheck without `@types/bun`.
interface WorkerScope {
  process: { env: Record<string, string | undefined> };
  Bun: { inspect(value: unknown): string };
  postMessage(message: unknown): void;
  addEventListener(type: "message", listener: (event: { data: HostMessage }) => void): void;
}
const scope = globalThis as unknown as WorkerScope;
const env = scope.process?.env ?? {};

const state = {
  cardOpen: false,
  settings: parseSettings(env.SIDEKICK_SETTINGS),
};

function parseSettings(json: string | undefined): Record<string, unknown> {
  try {
    return json ? JSON.parse(json) : {};
  } catch {
    return {};
  }
}

function send(message: PluginMessage) {
  scope.postMessage(message);
}

function describe(error: unknown) {
  return error instanceof Error ? (error.stack ?? error.message) : String(error);
}

/** Whether the widget's card is showing. Read it during render. */
export function useCardOpen(): boolean {
  return state.cardOpen;
}

/**
 * A value from the plugin's settings, as declared under `sidekick.settings`
 * in its `package.json` and edited in Settings › Plugins.
 */
export function useSetting<T = unknown>(key: string): T {
  return state.settings[key] as T;
}

/** Things a widget can ask the app to do. */
export const sidekick = {
  openUrl: (url: string) => send({ type: "open_url", url }),
  open: (path: string) => send({ type: "open_path", path }),
  copy: (text: string) => send({ type: "copy", text }),
  /** A folder the plugin can keep files in. */
  dataDir: env.SIDEKICK_DATA_DIR ?? "",
  /** The plugin's settings right now. */
  settings: () => ({ ...state.settings }),
};

export interface WidgetDefinition {
  /** Drawn in the dock slot, about 44 points square. Without it the
   * manifest's icon is shown. */
  tile?: Component;
  /** Drawn in the card that opens on hover. */
  card: Component;
}

/** Declares the widget and, inside the app, starts talking to it. */
export function widget(definition: WidgetDefinition): WidgetDefinition {
  if (env.SIDEKICK_PLUGIN === "1") start(definition);
  return definition;
}

function start(definition: WidgetDefinition) {
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

  const surfaces: Record<string, Component> = { card: definition.card };
  if (definition.tile) surfaces.tile = definition.tile;
  let sent = new Map<string, Node[]>();

  const render = () => {
    try {
      const output = renderSurfaces(surfaces);
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
      send({ type: "error", message: describe(error) });
    }
  };
  setInvalidateHandler(render);
  render();

  scope.addEventListener("message", ({ data: message }) => {
    switch (message.type) {
      case "event":
        try {
          dispatch(message.handler, message.value);
        } catch (error) {
          send({ type: "error", message: describe(error) });
        }
        break;
      case "card":
        state.cardOpen = message.open;
        break;
      case "settings":
        state.settings = message.values;
        break;
      case "resync":
        sent = new Map();
        break;
    }
    // Handlers usually change state; re-render in case they changed
    // something outside a hook. Unchanged surfaces send nothing.
    invalidate();
  });
}
