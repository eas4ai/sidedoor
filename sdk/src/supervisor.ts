// The one Bun process the app runs for all plugins. Each plugin gets its own
// Worker thread, so plugins can't see each other's state, and one that
// throws or hangs takes down only itself.
//
// The app and this process speak JSON lines tagged with a plugin id:
//   app → here:  {"type":"start","plugin":id,"entry":path,"data_dir":path,"settings":{…}}
//                {"type":"stop","plugin":id}
//                {"plugin":id, …a HostMessage for that plugin}
//   here → app:  {"plugin":id, …a PluginMessage}, and {"plugin":id,"type":"exited"}
//
// The app reloads a plugin by stopping and starting it. Before stopping, the
// worker is asked for its hook and store state, and a start soon after hands
// it to the new worker, so a save keeps what's on screen.

import { pathToFileURL } from "node:url";

interface Start {
  type: "start";
  plugin: string;
  entry: string;
  data_dir: string;
  settings: Record<string, unknown>;
}

type Line = Start | { type: "stop"; plugin: string } | { type: string; plugin: string };

interface Scope {
  process: {
    env: Record<string, string | undefined>;
    stdout: { write(text: string): void };
    exit(code: number): never;
  };
}
const { process } = globalThis as unknown as Scope;

export {};

const workers = new Map<string, Worker>();
/** Plugins being stopped, with the state they left, if any. */
const stopping = new Map<string, Promise<Kept | null>>();

interface Kept {
  state: unknown;
  at: number;
}

/** How long a stopping worker gets to hand over its state. */
const SNAPSHOT_WAIT = 300;
/** A start later than this after a stop is a fresh start, not a reload. */
const RELOAD_WINDOW = 5000;

function out(message: Record<string, unknown>) {
  process.stdout.write(`${JSON.stringify(message)}\n`);
}

async function start(line: Start) {
  stop(line.plugin);
  const { plugin } = line;
  const kept = await stopping.get(plugin);
  stopping.delete(plugin);
  const snapshot = kept && Date.now() - kept.at < RELOAD_WINDOW ? kept.state : null;
  const worker = new Worker(pathToFileURL(line.entry).href, {
    env: {
      ...process.env,
      SIDEDOOR_PLUGIN: "1",
      SIDEDOOR_PLUGIN_ID: plugin,
      SIDEDOOR_DATA_DIR: line.data_dir,
      SIDEDOOR_SETTINGS: JSON.stringify(line.settings ?? {}),
      ...(snapshot ? { SIDEDOOR_SNAPSHOT: JSON.stringify(snapshot) } : {}),
    },
  } as WorkerOptions);
  // Only the current worker speaks for the plugin; a stopped one is silent.
  const current = () => workers.get(plugin) === worker;
  worker.addEventListener("message", (event) => {
    if (current()) out({ ...(event.data as Record<string, unknown>), plugin });
  });
  worker.addEventListener("error", (event) => {
    event.preventDefault();
    if (current()) out({ plugin, type: "error", message: event.message });
  });
  worker.addEventListener("close", () => {
    if (current()) {
      workers.delete(plugin);
      out({ plugin, type: "exited" });
    }
  });
  workers.set(plugin, worker);
}

function stop(plugin: string) {
  const worker = workers.get(plugin);
  if (!worker) return;
  // From here the worker no longer speaks for the plugin; only its
  // snapshot reply is heard.
  workers.delete(plugin);
  const kept = new Promise<Kept | null>((resolve) => {
    const done = (state: unknown) => {
      clearTimeout(timer);
      worker.terminate();
      resolve(state ? { state, at: Date.now() } : null);
    };
    const timer = setTimeout(() => done(null), SNAPSHOT_WAIT);
    worker.addEventListener("message", (event) => {
      const data = event.data as { type?: string; state?: unknown };
      if (data.type === "snapshot") done(data.state);
    });
    worker.postMessage({ type: "snapshot" });
  });
  stopping.set(plugin, kept);
}

for await (const text of console as unknown as AsyncIterable<string>) {
  if (!text.trim()) continue;
  let line: Line;
  try {
    line = JSON.parse(text);
  } catch {
    continue;
  }
  if (line.type === "start") await start(line as Start);
  else if (line.type === "stop") stop(line.plugin);
  else {
    const { plugin, ...message } = line;
    workers.get(plugin)?.postMessage(message);
  }
}

// stdin closed: the app is gone.
process.exit(0);
