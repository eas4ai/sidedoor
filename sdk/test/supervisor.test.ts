import { expect, test } from "bun:test";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

test("supervisor loads a worker from a path containing spaces and URL punctuation", async () => {
  const directory = await mkdtemp(join(tmpdir(), "sidedoor # 100%-"));
  const entry = join(directory, "widget #1.ts");
  await writeFile(entry, 'postMessage({ type: "ready" });');
  const child = Bun.spawn([process.execPath, join(import.meta.dir, "../src/supervisor.ts")], {
    stdin: "pipe",
    stdout: "pipe",
    stderr: "pipe",
  });
  const timeout = setTimeout(() => child.kill(), 5000);
  try {
    child.stdin.write(`${JSON.stringify({
      type: "start",
      plugin: "test-path",
      entry,
      data_dir: directory,
      settings: {},
    })}\n`);
    child.stdin.flush();
    const reader = child.stdout.getReader();
    let output = "";
    while (!output.includes("\n")) {
      const chunk = await reader.read();
      if (chunk.done) break;
      output += new TextDecoder().decode(chunk.value);
    }
    reader.releaseLock();
    expect(JSON.parse(output.split("\n")[0]!)).toEqual({ type: "ready", plugin: "test-path" });
  } finally {
    clearTimeout(timeout);
    child.kill();
    await child.exited;
    await rm(directory, { recursive: true, force: true });
  }
}, 10000);
