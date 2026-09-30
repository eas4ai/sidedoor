// The clipboard history: what was copied, newest first, with the rules for
// repeats, limits, search and grouping. No I/O, so it's easy to test.

export type Clip =
  | { type: "text"; text: string }
  | { type: "link"; url: string }
  | { type: "image"; path: string; width: number; height: number; hash: string }
  | { type: "file"; path: string };

export interface Entry {
  id: number;
  clip: Clip;
  /** The app in front when it was copied. */
  source: string | null;
  /** Milliseconds since 1970. */
  copiedAt: number;
}

export interface History {
  entries: Entry[];
  nextId: number;
}

export const EMPTY: History = { entries: [], nextId: 0 };
export const MAX_ENTRIES = 200;
/** Images take room on disk; older ones make way. */
export const MAX_IMAGES = 12;

/** Text copied as a plain string is a link when it is one. */
export function fromText(text: string): Clip {
  const trimmed = text.trim();
  return /^https?:\/\//i.test(trimmed) && !/\s/.test(trimmed)
    ? { type: "link", url: trimmed }
    : { type: "text", text };
}

function sameContent(a: Clip, b: Clip): boolean {
  switch (a.type) {
    case "text":
      return b.type === "text" && a.text === b.text;
    case "link":
      return b.type === "link" && a.url === b.url;
    case "file":
      return b.type === "file" && a.path === b.path;
    case "image":
      return b.type === "image" && a.hash === b.hash;
  }
}

/**
 * Adds a copy. Copying something already in the history moves it to the
 * top instead. Returns image files that are no longer used, to delete.
 */
export function push(
  history: History,
  clip: Clip,
  source: string | null,
  now: number,
): { history: History; unused: string[] } {
  const index = history.entries.findIndex((entry) => sameContent(entry.clip, clip));
  if (index >= 0) {
    const existing = history.entries[index]!;
    const moved = { ...existing, copiedAt: now, source: source ?? existing.source };
    const entries = [moved, ...history.entries.filter((_, i) => i !== index)];
    // A repeated image arrives as a fresh file the history won't use.
    const unused =
      clip.type === "image" && existing.clip.type === "image" && clip.path !== existing.clip.path
        ? [clip.path]
        : [];
    return { history: { ...history, entries }, unused };
  }
  const id = history.nextId + 1;
  const entries = [{ id, clip, source, copiedAt: now }, ...history.entries];
  return trim({ entries, nextId: id });
}

function trim(history: History): { history: History; unused: string[] } {
  const unused: string[] = [];
  let images = 0;
  const entries = history.entries.filter((entry, index) => {
    const keep =
      index < MAX_ENTRIES && (entry.clip.type !== "image" || ++images <= MAX_IMAGES);
    if (!keep && entry.clip.type === "image") unused.push(entry.clip.path);
    return keep;
  });
  return { history: { ...history, entries }, unused };
}

/** Moves an entry to the top, as when it's copied again from the list. */
export function promote(history: History, id: number, now: number): History {
  const entry = history.entries.find((candidate) => candidate.id === id);
  if (!entry) return history;
  return {
    ...history,
    entries: [{ ...entry, copiedAt: now }, ...history.entries.filter((e) => e.id !== id)],
  };
}

/** Image files a history holds, to delete when it's cleared. */
export const images = (history: History) =>
  history.entries.flatMap((entry) => (entry.clip.type === "image" ? [entry.clip.path] : []));

const fileName = (path: string) => path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;

/** One line for lists. */
export function title(clip: Clip): string {
  switch (clip.type) {
    case "text":
      return clip.text.split("\n").map((line) => line.trim()).find(Boolean) ?? "";
    case "link":
      return clip.url;
    case "image":
      return `Image ${clip.width}×${clip.height}`;
    case "file":
      return fileName(clip.path);
  }
}

export function age(copiedAt: number, now: number): string {
  const seconds = Math.max(0, Math.floor((now - copiedAt) / 1000));
  if (seconds < 60) return "Just now";
  if (seconds < 3600) return `${Math.floor(seconds / 60)} min ago`;
  if (seconds < 86_400) return `${Math.floor(seconds / 3600)} h ago`;
  return `${Math.floor(seconds / 86_400)} d ago`;
}

export const FILTERS = ["All", "Text", "Links", "Images", "Files"] as const;
export type Filter = (typeof FILTERS)[number];

const KINDS: Record<Filter, Clip["type"] | null> = {
  All: null,
  Text: "text",
  Links: "link",
  Images: "image",
  Files: "file",
};

function searchable(entry: Entry): string {
  const { clip } = entry;
  const text =
    clip.type === "text"
      ? clip.text
      : clip.type === "link"
        ? clip.url
        : clip.type === "image"
          ? `image ${clip.width}×${clip.height} ${clip.width}x${clip.height}`
          : clip.path;
  return `${text} ${entry.source ?? ""}`.toLowerCase();
}

/** Entries of `filter`'s kind with every word of `query`, newest first. */
export function search(history: History, query: string, filter: Filter): Entry[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  const kind = KINDS[filter];
  return history.entries.filter(
    (entry) =>
      (kind === null || entry.clip.type === kind) &&
      words.every((word) => searchable(entry).includes(word)),
  );
}

/** "Today", "Yesterday" or "Earlier", in local time. */
export function day(copiedAt: number, now: number): "Today" | "Yesterday" | "Earlier" {
  const midnight = (time: number) => new Date(time).setHours(0, 0, 0, 0);
  const days = Math.round((midnight(now) - midnight(copiedAt)) / 86_400_000);
  return days <= 0 ? "Today" : days === 1 ? "Yesterday" : "Earlier";
}
