import { beforeEach, expect, test } from "bun:test";
import { mount } from "@sidedoor/sdk/testing";
import clipboard, { setCopier, setWatcher } from "./index";
import { EMPTY, MAX_IMAGES, day, fromText, push, search, title, type Clip } from "./history";
import { parseLine, type Copied } from "./watch";

let copy: (copied: Copied) => void = () => {};
const copiedBack: Clip[] = [];
beforeEach(() => {
  copiedBack.length = 0;
  setWatcher((_, onCopy) => {
    copy = onCopy;
    return () => {};
  });
  setCopier(async (clip) => {
    copiedBack.push(clip);
  });
});

const text = (value: string, source: string | null = "Notes"): Copied => ({ clip: fromText(value), source });

test("links are told apart from text", () => {
  expect(fromText(" https://example.com/a ")).toEqual({ type: "link", url: "https://example.com/a" });
  expect(fromText("see https://example.com").type).toBe("text");
  expect(title({ type: "text", text: "\n  first line \nsecond" })).toBe("first line");
  expect(title({ type: "file", path: "/Users/me/Project brief.pdf" })).toBe("Project brief.pdf");
});

test("copying something again moves it to the top", () => {
  let history = push(EMPTY, fromText("a"), "Notes", 1).history;
  history = push(history, fromText("b"), "Mail", 2).history;
  history = push(history, fromText("a"), null, 3).history;
  expect(history.entries.map((entry) => [title(entry.clip), entry.source, entry.copiedAt])).toEqual([
    ["a", "Notes", 3],
    ["b", "Mail", 2],
  ]);
});

test("old images make way and their files are freed", () => {
  let history = EMPTY;
  const unused: string[] = [];
  for (let n = 0; n <= MAX_IMAGES; n++) {
    const clip: Clip = { type: "image", path: `/img/${n}.png`, width: 1, height: 1, hash: String(n) };
    const pushed = push(history, clip, null, n);
    history = pushed.history;
    unused.push(...pushed.unused);
  }
  expect(history.entries).toHaveLength(MAX_IMAGES);
  expect(unused).toEqual(["/img/0.png"]);
  // The same picture copied again comes as a new file, which isn't kept.
  const again = push(history, { type: "image", path: "/img/new.png", width: 1, height: 1, hash: "5" }, null, 99);
  expect(again.unused).toEqual(["/img/new.png"]);
});

test("search needs every word, in the text or the app it came from", () => {
  let history = push(EMPTY, fromText("standup notes"), "Slack", 1).history;
  history = push(history, fromText("https://zed.dev"), "Safari", 2).history;
  expect(search(history, "notes SLACK", "All")).toHaveLength(1);
  expect(search(history, "zed", "Text")).toHaveLength(0);
  expect(search(history, "", "Links")).toHaveLength(1);
});

test("history groups by local day", () => {
  const now = new Date(2026, 8, 30, 12).getTime();
  expect(day(now - 3600_000, now)).toBe("Today");
  expect(day(new Date(2026, 8, 29, 23).getTime(), now)).toBe("Yesterday");
  expect(day(new Date(2026, 8, 20).getTime(), now)).toBe("Earlier");
});

test("the watcher's lines become copies", async () => {
  expect(await parseLine('{"kind":"text","text":"hi","source":"Notes"}')).toEqual({
    clip: { type: "text", text: "hi" },
    source: "Notes",
  });
  expect(await parseLine('{"kind":"file","path":"/etc/hosts","source":null}')).toEqual({
    clip: { type: "file", path: "/etc/hosts" },
    source: null,
  });
  expect(await parseLine('{"kind":"ready"}')).toBeNull();
  expect(await parseLine("not json")).toBeNull();
});

test("copies show on the card and copy back when clicked", async () => {
  const plugin = mount(clipboard);
  expect(plugin.text()).toContain("will appear here");
  copy(text("first"));
  copy(text("https://example.com", "Safari"));
  await plugin.settle();
  expect(plugin.text()).toContain("2 copied");
  expect(plugin.text("tile")).toContain("2");
  await plugin.press("first");
  expect(copiedBack).toEqual([{ type: "text", text: "first" }]);
  // It moved to the top and is kept across restarts.
  const saved = plugin.storage.get("history") as { entries: Array<{ clip: Clip }> };
  expect(title(saved.entries[0]!.clip)).toBe("first");
  plugin.unmount();
});

test("clearing takes a second click", async () => {
  const plugin = mount(clipboard);
  copy(text("secret-ish"));
  await plugin.settle();
  await plugin.press("Clear History");
  expect(plugin.text()).toContain("1 copied");
  await plugin.press("Click Again to Clear");
  expect(plugin.text()).toContain("will appear here");
  plugin.unmount();
});

test("the history window searches everything and closes after a copy", async () => {
  const plugin = mount(clipboard);
  copy(text("quarterly report"));
  copy(text("lunch order"));
  await plugin.click();
  await plugin.settle();
  expect(plugin.text("window:history")).toContain("quarterly report");
  await plugin.find((node) => node.type === "Input", "window:history").change("lunch");
  expect(plugin.text("window:history")).not.toContain("quarterly report");
  await plugin.press("lunch order", "window:history");
  expect(copiedBack).toEqual([{ type: "text", text: "lunch order" }]);
  expect(plugin.sent.some((message) => message.type === "close_window")).toBe(true);
  plugin.unmount();
});
