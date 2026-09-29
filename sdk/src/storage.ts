// A plugin's saved values: one JSON file in its data folder, so state
// survives reloads and restarts.

interface Fs {
  readFileSync(path: string, encoding: "utf8"): string;
  writeFileSync(path: string, data: string): void;
  renameSync(from: string, to: string): void;
}

interface Scope {
  process?: { getBuiltinModule?(name: "node:fs"): Fs };
}

export interface Storage {
  get<T = unknown>(key: string): T | undefined;
  has(key: string): boolean;
  set(key: string, value: unknown): void;
  delete(key: string): void;
}

/**
 * Values kept in `file`, or only in memory when there is no file (as in
 * tests). `onChange` runs after every change.
 */
export function createStorage(file: string | null, onChange: () => void = () => {}): Storage {
  const fs = file ? (globalThis as unknown as Scope).process?.getBuiltinModule?.("node:fs") : undefined;
  let values: Record<string, unknown> | null = null;
  let saving = false;

  const load = (): Record<string, unknown> => {
    if (values) return values;
    let loaded: Record<string, unknown> = {};
    if (fs && file) {
      try {
        const parsed = JSON.parse(fs.readFileSync(file, "utf8"));
        if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) loaded = parsed;
      } catch {
        // No file yet, or one that isn't ours: start empty.
      }
    }
    values = loaded;
    return loaded;
  };

  // Changes in one tick are written together, before the tick ends, so a
  // reload right after a change doesn't lose it.
  const save = () => {
    if (!fs || !file || saving) return;
    saving = true;
    queueMicrotask(() => {
      saving = false;
      const temp = `${file}.tmp`;
      fs.writeFileSync(temp, JSON.stringify(load(), null, 2));
      fs.renameSync(temp, file);
    });
  };

  const change = () => {
    save();
    onChange();
  };

  const remove = (key: string) => {
    if (!(key in load())) return;
    delete load()[key];
    change();
  };

  return {
    get: <T>(key: string) => load()[key] as T | undefined,
    has: (key) => key in load(),
    set(key, value) {
      if (value === undefined) return remove(key);
      load()[key] = value;
      change();
    },
    delete: remove,
  };
}
