// The one Bun process the app runs for all plugins. Each plugin gets its own
// Worker thread, so plugins can't see each other's state, and one that
// throws or hangs takes down only itself.
//
// The app and this process speak JSON lines tagged with a plugin id:
//   app → here:  {"type":"start","plugin":id,"entry":path,"data_dir":path,"settings":{…}}
//                {"type":"stop","plugin":id}
//                {"plugin":id, …a HostMessage for that plugin}
//   here → app:  {"plugin":id, …a PluginMessage}, and {"plugin":id,"type":"exited"}

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

function out(message: Record<string, unknown>) {
  process.stdout.write(`${JSON.stringify(message)}\n`);
}

function start(line: Start) {
  stop(line.plugin);
  const { plugin } = line;
  const worker = new Worker(new URL(`file://${line.entry}`).href, {
    env: {
      ...process.env,
      SIDEKICK_PLUGIN: "1",
      SIDEKICK_PLUGIN_ID: plugin,
      SIDEKICK_DATA_DIR: line.data_dir,
      SIDEKICK_SETTINGS: JSON.stringify(line.settings ?? {}),
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
  workers.delete(plugin);
  worker?.terminate();
}

for await (const text of console as unknown as AsyncIterable<string>) {
  if (!text.trim()) continue;
  let line: Line;
  try {
    line = JSON.parse(text);
  } catch {
    continue;
  }
  if (line.type === "start") start(line as Start);
  else if (line.type === "stop") stop(line.plugin);
  else {
    const { plugin, ...message } = line;
    workers.get(plugin)?.postMessage(message);
  }
}

// stdin closed: the app is gone.
process.exit(0);
