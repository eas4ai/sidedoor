// Reads the CPU, memory and startup disk with Bun, the way Activity Monitor
// and Finder count them.

import { statfsSync } from "node:fs";
import { cpus, freemem, platform, totalmem } from "node:os";

export interface Snapshot {
  /** 0–100, across all cores, since the previous sample. */
  cpu: number;
  memoryUsed: number;
  memoryTotal: number;
  diskUsed: number;
  diskTotal: number;
}

export interface CpuTimes {
  busy: number;
  total: number;
}

export function cpuTimes(): CpuTimes {
  let busy = 0;
  let total = 0;
  for (const { times } of cpus()) {
    const used = times.user + times.nice + times.sys + times.irq;
    busy += used;
    total += used + times.idle;
  }
  return { busy, total };
}

/** Percent busy between two readings. */
export function cpuPercent(before: CpuTimes, after: CpuTimes): number {
  const total = after.total - before.total;
  if (total <= 0) return 0;
  return Math.min(100, Math.max(0, ((after.busy - before.busy) / total) * 100));
}

/**
 * Memory in use from `vm_stat`, as Activity Monitor counts it: app memory,
 * wired and compressed. `os.freemem()` on a Mac leaves out the cache macOS
 * gives back on demand, so it would show memory as nearly full.
 */
export function macMemoryUsed(vmStat: string): number | null {
  const pageSize = Number(vmStat.match(/page size of (\d+) bytes/)?.[1]);
  const pages = (label: string) =>
    Number(vmStat.match(new RegExp(`${label}:\\s+(\\d+)`))?.[1] ?? NaN);
  const anonymous = pages("Anonymous pages");
  const purgeable = pages("Pages purgeable");
  const wired = pages("Pages wired down");
  const compressed = pages("Pages occupied by compressor");
  const used = anonymous - purgeable + wired + compressed;
  return Number.isFinite(used) && pageSize > 0 ? used * pageSize : null;
}

/** Memory in use on Linux: what isn't available to start new programs. */
export function linuxMemoryUsed(meminfo: string): number | null {
  const kb = (label: string) => Number(meminfo.match(new RegExp(`${label}:\\s+(\\d+) kB`))?.[1]);
  const used = kb("MemTotal") - kb("MemAvailable");
  return Number.isFinite(used) ? used * 1024 : null;
}

async function memoryUsed(): Promise<number> {
  try {
    if (platform() === "darwin") {
      const vmStat = await new Response(Bun.spawn(["vm_stat"]).stdout).text();
      const used = macMemoryUsed(vmStat);
      if (used !== null) return used;
    } else if (platform() === "linux") {
      const used = linuxMemoryUsed(await Bun.file("/proc/meminfo").text());
      if (used !== null) return used;
    }
  } catch {
    // Fall back to what Node reports.
  }
  return totalmem() - freemem();
}

function disk(): { used: number; total: number } {
  const root = platform() === "win32" ? `${process.env.SystemDrive ?? "C:"}\\` : "/";
  try {
    const { blocks, bavail, bsize } = statfsSync(root);
    const total = blocks * bsize;
    return { used: total - bavail * bsize, total };
  } catch {
    return { used: 0, total: 0 };
  }
}

/** Takes a sample; `before` is the previous CPU reading. */
export async function sample(before: CpuTimes): Promise<{ snapshot: Snapshot; times: CpuTimes }> {
  const times = cpuTimes();
  const { used, total } = disk();
  return {
    times,
    snapshot: {
      cpu: cpuPercent(before, times),
      memoryUsed: await memoryUsed(),
      memoryTotal: totalmem(),
      diskUsed: used,
      diskTotal: total,
    },
  };
}

export const percent = (used: number, total: number) => (total > 0 ? (used / total) * 100 : 0);

const gb = (value: number) => (value >= 100 ? `${value.toFixed(0)} GB` : `${value.toFixed(1)} GB`);
/** Disk sizes in decimal gigabytes, as Finder shows them. */
export const formatBytes = (bytes: number) => gb(bytes / 1e9);
/** Memory in binary gigabytes, as Activity Monitor shows it. */
export const formatMemory = (bytes: number) => gb(bytes / 2 ** 30);
