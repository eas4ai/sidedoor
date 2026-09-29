import { afterEach, expect, test } from "bun:test";
import {
  definePlugin,
  useData,
  type ClipboardData,
  type WeatherData,
} from "../src";
import { mount } from "../src/testing";
import { reset } from "../src/runtime";
import weather from "../../crates/desktop/src/builtins/weather";
import stats from "../../crates/desktop/src/builtins/stats";
import clipboard from "../../crates/desktop/src/builtins/clipboard";

afterEach(reset);

const forecast: WeatherData = {
  status: "ready",
  location: { name: "Aalborg", latitude: 57.048, longitude: 9.919 },
  conditionLabel: "Clear",
  updatedMinutes: 2,
  weather: {
    temperature: -1.5,
    high: 3,
    low: -4,
    condition: "clear",
    is_day: false,
    hours: [
      { hour: 5, temperature: -2, condition: "clear" },
      { hour: 6, temperature: 1, condition: "partly_cloudy" },
    ],
  },
};

test("weather keeps its dimensions, labels and day/night icons through live updates", async () => {
  const plugin = mount(weather);
  expect(plugin.sent[0]).toMatchObject({
    type: "manifest",
    width: 300,
    height: 190,
    data: ["weather"],
  });
  expect(plugin.text("tile")).toBe("--°");
  await plugin.setData("weather", forecast);
  expect(plugin.text("tile")).toBe("-2°");
  expect(plugin.text()).toContain("High 3°, low -4°");
  expect(plugin.text()).toContain("Updated 2 min ago");
  expect(plugin.findAll("Icon").map((icon) => icon.props.name)).toEqual([
    "moon",
    "moon",
    "cloud-sun",
  ]);
  await plugin.setData("weather", {
    status: "failed",
    location: forecast.location,
    message: "Offline",
  });
  expect(plugin.text()).toContain("Offline");
  expect(plugin.findAll("Icon", "tile")[0]?.props.name).toBe("cloud-off");
  await plugin.setData("weather", {
    status: "loading",
    location: { ...forecast.location, name: "Copenhagen" },
  });
  expect(plugin.text()).toContain("CopenhagenLoading weather…");
});

test("stats sends native animation targets and its thirty-sample history", async () => {
  const plugin = mount(stats);
  expect(plugin.text()).toContain(`Reading your ${process.platform === "win32" ? "PC" : "Mac"}…`);
  await plugin.setData("stats", {
    cpu: 24,
    memoryPercent: 60,
    diskPercent: 70,
    memoryUsed: "12.0 GB",
    memoryTotal: "20.0 GB",
    diskFree: "300 GB",
    history: [10, 24],
    interval: 2,
  });
  expect(
    plugin.findAll("NumberText", "tile").map((node) => node.props.value),
  ).toEqual([24, 60]);
  expect(plugin.findAll("Meter").map((node) => node.props.fraction)).toEqual([
    0.24, 0.6, 0.7,
  ]);
  expect(plugin.findAll("Meter").every((node) => node.props.animated)).toBe(
    true,
  );
  expect(plugin.findAll("Sparkline")[0]?.props.values).toEqual([0.1, 0.24]);
  expect(plugin.text()).toContain("Refreshes every 2 s");
});

const copies: ClipboardData = {
  count: 3,
  clearArmed: false,
  entries: [
    {
      id: 8,
      title: "Notes",
      kind: { type: "text", text: "Notes" },
      age: "Just now",
      source: "Notes",
    },
    {
      id: 7,
      title: "Image 20×30",
      kind: { type: "image", path: "/tmp/sample.png", width: 20, height: 30 },
      age: "1 min ago",
      source: null,
    },
    {
      id: 6,
      title: "hello.txt",
      kind: { type: "file", path: "/tmp/hello.txt" },
      age: "2 min ago",
      source: "Finder",
    },
  ],
};

test("clipboard fits its rows, keeps previews, and delegates actions to native services", async () => {
  const plugin = mount(clipboard);
  expect(plugin.findAll("Card")[0]?.props.h).toBe(112);
  await plugin.setData("clipboard", copies);
  expect(plugin.findAll("Card")[0]?.props.h).toBe(230);
  expect(plugin.findAll("img")[0]?.props).toMatchObject({
    src: "/tmp/sample.png",
    size: 28,
    object_fit: "cover",
  });
  expect(plugin.text()).toContain("Just now · Notes");
  await plugin.find((node) => node.props.id === "clip:7").click();
  await plugin.press("Show All");
  await plugin.click();
  await plugin.press("Clear History");
  expect(plugin.sent.filter((message) => message.type === "clipboard")).toEqual(
    [
      { type: "clipboard", action: "copy_entry", id: 7 },
      { type: "clipboard", action: "show_history" },
      { type: "clipboard", action: "show_history" },
      { type: "clipboard", action: "request_clear" },
    ],
  );
  await plugin.setData("clipboard", { ...copies, clearArmed: true });
  expect(
    plugin.find((node) => node.props.id === "clear-history").props.text_color,
  ).toBe("red");
  await plugin.press("Click Again to Clear");
  await plugin.setData("clipboard", {
    count: 0,
    entries: [],
    clearArmed: false,
  });
  expect(plugin.findAll("Card")[0]?.props.h).toBe(112);
  expect(plugin.text("tile")).toBe("");
});

test("live feeds are opt-in and a new plugin session starts without old data", async () => {
  const reader = mount(
    definePlugin({
      name: "Reader",
      data: ["clipboard"],
      card: () => String(useData("clipboard")?.count ?? 0),
    }),
  );
  await reader.setData("clipboard", copies);
  expect(reader.text()).toBe("3");
  reader.unmount();
  const other = mount(
    definePlugin({
      name: "Other",
      card: () => String(useData("clipboard")?.count ?? 0),
    }),
  );
  await other.setData("clipboard", copies);
  expect(other.text()).toBe("0");
});
