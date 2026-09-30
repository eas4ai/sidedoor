import { expect, test } from "bun:test";
import { mount } from "@sidedoor/sdk/testing";
import stats from "./index";
import { cpuPercent, formatBytes, formatMemory, linuxMemoryUsed, macMemoryUsed } from "./sample";

test("sizes read like Finder and Activity Monitor", () => {
  expect(formatBytes(19_500_000_000)).toBe("19.5 GB");
  expect(formatBytes(482_000_000_000)).toBe("482 GB");
  expect(formatMemory(24 * 2 ** 30)).toBe("24.0 GB");
});

test("CPU is the busy share of time between two readings", () => {
  expect(cpuPercent({ busy: 100, total: 400 }, { busy: 150, total: 600 })).toBe(25);
  expect(cpuPercent({ busy: 1, total: 1 }, { busy: 1, total: 1 })).toBe(0);
});

test("memory in use is read from vm_stat and /proc/meminfo", () => {
  const vmStat = `Mach Virtual Memory Statistics: (page size of 16384 bytes)
Pages free:                               10000.
Anonymous pages:                         500000.
Pages purgeable:                          20000.
Pages wired down:                        100000.
Pages occupied by compressor:             30000.`;
  expect(macMemoryUsed(vmStat)).toBe((500000 - 20000 + 100000 + 30000) * 16384);
  expect(macMemoryUsed("nonsense")).toBeNull();
  expect(linuxMemoryUsed("MemTotal:  1000 kB\nMemAvailable:  250 kB\n")).toBe(750 * 1024);
});

test("the card fills in once the first sample lands", async () => {
  const plugin = mount(stats);
  expect(plugin.text("card")).toContain("Reading your");
  await plugin.waitFor(() => expect(plugin.findAll("Meter")).toHaveLength(3), { timeout: 3000 });
  const storage = plugin.findAll("Meter").find((meter) => meter.props.id === "storage");
  expect(String(storage?.props.value)).toMatch(/ GB free$/);
  plugin.unmount();
});
