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
import type { Component, IconName, Node } from "./types";

/** Messages the app sends a plugin. */
export type HostMessage =
  | { type: "event"; handler: string; value?: unknown }
  | { type: "card"; open: boolean }
  | { type: "settings"; values: Record<string, unknown> }
  /** The app lost track of a surface; send everything again. */
  | { type: "resync" };

/** Messages a plugin sends the app. */
export type PluginMessage =
  | ({ type: "manifest" } & Manifest)
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
  /** What the user saved; defaults fill the gaps. */
  saved: parseSettings(env.SIDEKICK_SETTINGS),
  defaults: {} as Record<string, unknown>,
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

function describeError(error: unknown) {
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
  return (key in state.saved ? state.saved[key] : state.defaults[key]) as T;
}

/** Things a widget can ask the app to do. */
export const sidekick = {
  openUrl: (url: string) => send({ type: "open_url", url }),
  open: (path: string) => send({ type: "open_path", path }),
  copy: (text: string) => send({ type: "copy", text }),
  /** A folder the plugin can keep files in. */
  dataDir: env.SIDEKICK_DATA_DIR ?? "",
  /** The plugin's settings right now. */
  settings: (): Record<string, unknown> => ({ ...state.defaults, ...state.saved }),
};

/** A setting the user can change in Settings › Plugins. */
export type SettingDefinition = {
  title: string;
  description?: string;
} & (
  | { type: "text" | "secret"; default?: string }
  | { type: "number"; default?: number }
  | { type: "toggle"; default?: boolean }
  | { type: "choice"; options: string[]; default?: string }
);

export interface PluginDefinition {
  /** Shown in the dock's menus and in Settings. */
  name: string;
  /** A Lucide icon name, e.g. `"timer"`; the dock shows it until `tile` draws. */
  icon?: IconName;
  /** Card width in points; 280 by default. */
  width?: number;
  /** Card height in points. Leave it out and the card fits its content. */
  height?: number;
  /** Settings by key, read with `useSetting(key)`. */
  settings?: Record<string, SettingDefinition>;
  /** Drawn in the dock slot, about 44 points square. */
  tile?: Component;
  /** Drawn in the card that opens on hover. */
  card: Component;
}

/** What the app learns about a plugin when it starts. */
export interface Manifest {
  name: string;
  icon: string;
  width: number;
  height: number | null;
  settings: Array<{ key: string } & SettingDefinition>;
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
export function describe(definition: PluginDefinition): {
  manifest: Manifest;
  defaults: Record<string, unknown>;
} {
  const settings = Object.entries(definition.settings ?? {});
  return {
    manifest: {
      name: definition.name,
      icon: definition.icon ?? "puzzle",
      width: definition.width ?? 280,
      height: definition.height ?? null,
      settings: settings.map(([key, setting]) => ({ key, ...setting })),
    },
    defaults: Object.fromEntries(settings.map(([key, setting]) => [key, defaultOf(setting)])),
  };
}

/**
 * Declares the plugin: what it's called, how it looks in the dock and its
 * settings, all in one place. Inside the app it also starts the plugin.
 */
export function definePlugin(definition: PluginDefinition): PluginDefinition {
  if (env.SIDEKICK_PLUGIN === "1") start(definition);
  return definition;
}

function start(definition: PluginDefinition) {
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

  const { manifest, defaults } = describe(definition);
  state.defaults = defaults;
  send({ type: "manifest", ...manifest });

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
      send({ type: "error", message: describeError(error) });
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
          send({ type: "error", message: describeError(error) });
        }
        break;
      case "card":
        state.cardOpen = message.open;
        break;
      case "settings":
        state.saved = message.values;
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
