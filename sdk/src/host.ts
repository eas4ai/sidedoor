// The bridge to the app. The host runs each plugin as its own Bun process
// and speaks JSON lines: rendered surfaces go out on stdout, events come in
// on stdin. Plugin logging goes to stderr so it never mixes with the protocol.

import {
  dispatch,
  invalidate,
  renderSurfaces,
  runEffects,
  setInvalidateHandler,
} from "./runtime";
import type { Component, Node } from "./types";

/** Messages the host sends. */
export type HostMessage = { type: "event"; handler: string; value?: unknown };

/** Messages a plugin sends. */
export type PluginMessage =
  | { type: "render"; surface: string; tree: Node[] }
  | { type: "open_url"; url: string }
  | { type: "open_path"; path: string }
  | { type: "copy"; text: string }
  | { type: "error"; message: string };

// Bun's globals, typed here so plugins typecheck without `@types/bun`.
interface Runtime {
  process: {
    env: Record<string, string | undefined>;
    stdout: { write(text: string): void };
    stderr: { write(text: string): void };
    exit(code: number): never;
  };
  Bun: { inspect(value: unknown): string };
}
const { process, Bun } = globalThis as unknown as Runtime;
/** Bun reads stdin line by line by iterating `console`. */
const stdinLines = console as unknown as AsyncIterable<string>;

function send(message: PluginMessage) {
  process.stdout.write(`${JSON.stringify(message)}\n`);
}

/** Things a widget can ask the app to do. */
export const sidekick = {
  openUrl: (url: string) => send({ type: "open_url", url }),
  open: (path: string) => send({ type: "open_path", path }),
  copy: (text: string) => send({ type: "copy", text }),
  /** A folder the plugin can keep files in. */
  dataDir: process.env.SIDEKICK_DATA_DIR ?? "",
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
  if (process.env.SIDEKICK_PLUGIN === "1") start(definition);
  return definition;
}

function start(definition: WidgetDefinition) {
  // Keep stdout for the protocol.
  const log = (...args: unknown[]) =>
    process.stderr.write(`${args.map((arg) => (typeof arg === "string" ? arg : Bun.inspect(arg))).join(" ")}\n`);
  console.log = log;
  console.info = log;
  console.debug = log;

  const surfaces: Record<string, Component> = { card: definition.card };
  if (definition.tile) surfaces.tile = definition.tile;
  const sent = new Map<string, string>();

  const render = () => {
    try {
      const output = renderSurfaces(surfaces);
      for (const [surface, tree] of Object.entries(output)) {
        const json = JSON.stringify(tree);
        if (sent.get(surface) === json) continue;
        sent.set(surface, json);
        send({ type: "render", surface, tree });
      }
      runEffects();
    } catch (error) {
      send({ type: "error", message: error instanceof Error ? error.stack ?? error.message : String(error) });
    }
  };
  setInvalidateHandler(render);
  render();

  (async () => {
    for await (const line of stdinLines) {
      if (!line.trim()) continue;
      let message: HostMessage;
      try {
        message = JSON.parse(line);
      } catch {
        continue;
      }
      if (message.type === "event") {
        try {
          dispatch(message.handler, message.value);
        } catch (error) {
          send({ type: "error", message: error instanceof Error ? error.stack ?? error.message : String(error) });
        }
      }
      // Handlers usually change state; re-render in case they changed
      // something outside a hook.
      invalidate();
    }
    process.exit(0);
  })();
}
