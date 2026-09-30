// Watches the system clipboard with the tools each system has, and copies
// back to it. macOS gets everything (text, links, files, images, and it
// skips passwords); Linux and Windows get text.

import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { fromText, type Clip } from "./history";

export interface Copied {
  clip: Clip;
  source: string | null;
}

type Line =
  | { kind: "ready" | "error"; message?: string }
  | { kind: "text"; text: string; source: string | null }
  | { kind: "file"; path: string; source: string | null }
  | { kind: "image"; path: string; width: string | number; height: string | number; source: string | null };

/** What a line from `pasteboard.js` means. */
export async function parseLine(line: string): Promise<Copied | null> {
  let value: Line;
  try {
    value = JSON.parse(line);
  } catch {
    return null;
  }
  switch (value.kind) {
    case "text":
      return { clip: fromText(value.text), source: value.source };
    case "file":
      return { clip: { type: "file", path: value.path }, source: value.source };
    case "image": {
      const bytes = await Bun.file(value.path).arrayBuffer();
      return {
        clip: {
          type: "image",
          path: value.path,
          width: Number(value.width),
          height: Number(value.height),
          hash: Bun.hash(bytes).toString(16),
        },
        source: value.source,
      };
    }
    default:
      return null;
  }
}

async function lines(stream: ReadableStream<Uint8Array>, each: (line: string) => Promise<void>) {
  const decoder = new TextDecoder();
  let buffer = "";
  for await (const chunk of stream) {
    buffer += decoder.decode(chunk, { stream: true });
    let end: number;
    while ((end = buffer.indexOf("\n")) >= 0) {
      await each(buffer.slice(0, end));
      buffer = buffer.slice(end + 1);
    }
  }
}

/** The macOS watcher runs for an hour at a time, then starts afresh. */
const MAC_LIFETIME = 60 * 60_000;

function watchMac(images: string, onCopy: (copied: Copied) => void): () => void {
  let stopped = false;
  let current: ReturnType<typeof Bun.spawn> | null = null;
  const start = () => {
    if (stopped) return;
    const proc = Bun.spawn(["osascript", "-l", "JavaScript", join(import.meta.dir, "pasteboard.js"), images], {
      stdout: "pipe",
      stderr: "ignore",
    });
    current = proc;
    const restart = setTimeout(() => proc.kill(), MAC_LIFETIME);
    void lines(proc.stdout, async (line) => {
      const copied = await parseLine(line);
      if (copied) onCopy(copied);
    });
    void proc.exited.then(() => {
      clearTimeout(restart);
      // Back soon after a crash or the hourly restart.
      if (!stopped) setTimeout(start, 1000);
    });
  };
  start();
  return () => {
    stopped = true;
    current?.kill();
  };
}

/** Polls text on Linux and Windows. */
function watchText(read: () => Promise<string | null>, onCopy: (copied: Copied) => void, every: number) {
  let last: string | null | undefined;
  let busy = false;
  const timer = setInterval(async () => {
    if (busy) return;
    busy = true;
    try {
      const text = await read();
      // The first reading is what was there before we started.
      if (last !== undefined && text && text !== last) onCopy({ clip: fromText(text), source: null });
      last = text;
    } finally {
      busy = false;
    }
  }, every);
  return () => clearInterval(timer);
}

async function output(command: string[]): Promise<string | null> {
  try {
    const proc = Bun.spawn(command, { stdout: "pipe", stderr: "ignore" });
    const [text, code] = await Promise.all([new Response(proc.stdout).text(), proc.exited]);
    return code === 0 ? text : null;
  } catch {
    return null;
  }
}

/** Starts watching; returns a function that stops. */
export function watch(dataDir: string, onCopy: (copied: Copied) => void): () => void {
  if (process.platform === "darwin") {
    const images = join(dataDir, "images");
    mkdirSync(images, { recursive: true });
    return watchMac(images, onCopy);
  }
  if (process.platform === "win32")
    return watchText(
      () => output(["powershell", "-NoProfile", "-Command", "Get-Clipboard -Raw"]),
      onCopy,
      1500,
    );
  const wayland = Boolean(process.env.WAYLAND_DISPLAY);
  return watchText(
    () => output(wayland ? ["wl-paste", "--no-newline"] : ["xclip", "-selection", "clipboard", "-o"]),
    onCopy,
    1000,
  );
}

/** JXA that puts a file or an image back on the macOS pasteboard. */
const WRITE_MAC = `
ObjC.import("AppKit");
function run(argv) {
  const [kind, path] = argv;
  const pasteboard = $.NSPasteboard.generalPasteboard;
  pasteboard.clearContents;
  const object = kind === "image"
    ? $.NSImage.alloc.initWithContentsOfFile(path)
    : $.NSURL.fileURLWithPath(path);
  pasteboard.writeObjects($([object]));
}`;

/** Copies `clip` back to the clipboard. Text goes through the app. */
export async function copyBack(clip: Clip, copyText: (text: string) => void) {
  if (clip.type === "text") return copyText(clip.text);
  if (clip.type === "link") return copyText(clip.url);
  if (process.platform === "darwin") {
    await Bun.spawn(["osascript", "-l", "JavaScript", "-e", WRITE_MAC, clip.type, clip.path]).exited;
  } else {
    // Other systems get the file's path.
    copyText(clip.path);
  }
}
