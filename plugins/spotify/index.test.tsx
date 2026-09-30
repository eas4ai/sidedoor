import { beforeEach, expect, setSystemTime, test } from "bun:test";
import { mount } from "@sidedoor/sdk/testing";
import plugin from "./index";
import { bmpPixels, pickTint, setExtractor } from "./colors";
import { setRunner, setWatcher, webUrl } from "./spotify";

/** A fake Spotify that answers the read script and records commands. */
let app: {
  running: boolean;
  state: "playing" | "paused" | "stopped";
  volume: number;
  shuffling: boolean;
  position: number;
  url: string;
  denied?: boolean;
};
let commands: string[];
/** What the plugin's watcher would call when Spotify posts a change. */
let announce: () => void;

beforeEach(() => {
  app = {
    running: true,
    state: "paused",
    volume: 50,
    shuffling: false,
    position: 65,
    url: "spotify:track:123",
  };
  commands = [];
  setExtractor(async () => ({
    from: "#aa3322",
    to: "#662211",
    accent: "#e05a44",
    onAccent: "#000000",
  }));
  announce = () => {};
  setWatcher((onChange) => {
    announce = onChange;
    return () => {};
  });
  setRunner(async (script) => {
    if (script.includes("JSON.stringify")) {
      if (app.denied) return JSON.stringify({ running: false, denied: true });
      if (!app.running) return JSON.stringify({ running: false });
      return JSON.stringify({
        running: true,
        state: app.state,
        track:
          app.state === "stopped"
            ? null
            : {
                name: "Song",
                artist: "Artist",
                album: "Album",
                duration: 200_000,
                artwork: "https://i.scdn.co/image/abc",
                url: app.url,
              },
        position: app.position,
        volume: app.volume,
        shuffling: app.shuffling,
        repeating: false,
      });
    }
    commands.push(script);
    if (script.includes("playpause")) app.state = app.state === "playing" ? "paused" : "playing";
    const volume = script.match(/soundVolume = (\d+)/);
    if (volume) app.volume = Number(volume[1]);
    if (script.includes("shuffling = true")) app.shuffling = true;
    const seek = script.match(/playerPosition = ([\d.]+)/);
    if (seek) app.position = Number(seek[1]);
    return "";
  });
});

/** Mounts the plugin and waits for its first read of Spotify. */
const start = async (options?: Parameters<typeof mount>[1]) => {
  const p = mount(plugin, options);
  await p.settle();
  return p;
};
const byId = (p: ReturnType<typeof mount>, id: string) =>
  p.find((element) => element.props.id === id);

test("shows the current track", async () => {
  const p = await start();
  expect(p.find("Spotify").props.accessory).toBe("Paused");
  expect(p.text()).toContain("Song");
  expect(p.text()).toContain("Artist");
  expect(p.text()).toContain("1:05");
  expect(p.text()).toContain("-2:15");
  p.unmount();
});

test("clicking the end time switches between time left and length", async () => {
  const p = await start();
  await byId(p, "end-time").click();
  expect(p.text()).toContain("3:20");
  expect(p.text()).not.toContain("-2:15");
  expect(p.storage.get<boolean>("showRemaining")).toBe(false);
  await byId(p, "end-time").click();
  expect(p.text()).toContain("-2:15");
  p.unmount();
});

test("the play button toggles playback", async () => {
  const p = await start();
  await p.press("Play");
  expect(commands.some((c) => c.includes("playpause"))).toBe(true);
  expect(p.find("Spotify").props.accessory).toBe("Playing");
  expect(() => p.find("Pause")).not.toThrow();
  p.unmount();
});

test("clicking the dock item follows the setting", async () => {
  const p = await start({ settings: { click: "Next Track" } });
  await p.click();
  expect(commands.some((c) => c.includes("nextTrack"))).toBe(true);
  p.unmount();
});

test("the seek slider jumps to where it's let go", async () => {
  const p = await start();
  expect(byId(p, "seek").props.value).toBeCloseTo(65 / 200);
  await byId(p, "seek").change(0.5);
  await p.waitFor(() => expect(app.position).toBe(100));
  expect(p.text()).toContain("1:40");
  expect(byId(p, "seek").props.value).toBeCloseTo(0.5);
  p.unmount();
});

test("the volume slider changes volume as it drags", async () => {
  const p = await start();
  expect(byId(p, "volume").props.value).toBe(0.5);
  await byId(p, "volume").change(0.8);
  await p.waitFor(() => expect(app.volume).toBe(80));
  expect(byId(p, "volume").props.value).toBe(0.8);
  p.unmount();
});

test("shuffle turns on", async () => {
  const p = await start();
  await p.press("Shuffle");
  expect(app.shuffling).toBe(true);
  p.unmount();
});

test("offers to open Spotify when it's closed", async () => {
  app.running = false;
  const p = await start();
  expect(p.text()).toContain("Spotify isn't running.");
  await p.press("Open Spotify");
  expect(commands.some((c) => c.includes("activate"))).toBe(true);
  p.unmount();
});

test("Copy Link copies the web URL", async () => {
  const p = await start();
  await p.press("Copy Link");
  expect(JSON.stringify(p.sent)).toContain("https://open.spotify.com/track/123");
  expect(webUrl("spotify:episode:9")).toBe("https://open.spotify.com/episode/9");
  p.unmount();
});

test("episodes swap shuffle and repeat for seek buttons", async () => {
  app.url = "spotify:episode:9";
  const p = await start();
  expect(() => p.find("Shuffle")).toThrow();
  await p.press("Forward 15 Seconds");
  expect(app.position).toBe(80);
  await p.press("Back 15 Seconds");
  await p.press("Back 15 Seconds");
  expect(app.position).toBe(50);
  expect(p.text()).toContain("0:50");
  p.unmount();
});

test("seeking stays inside the episode", async () => {
  app.url = "spotify:episode:9";
  app.position = 5;
  const p = await start();
  await p.press("Back 15 Seconds");
  expect(app.position).toBe(0);
  await p.action("forward");
  expect(app.position).toBe(15);
  p.unmount();
});

test("music keeps shuffle and repeat", async () => {
  const p = await start();
  expect(() => p.find("Forward 15 Seconds")).toThrow();
  expect(p.find("Repeat")).toBeDefined();
  p.unmount();
});

test("the card is tinted with the artwork's colors", async () => {
  const p = await start();
  await p.waitFor(() =>
    expect(p.find("Spotify").props.bg_gradient).toEqual({
      from: "#aa332273",
      to: "#6622112e",
      angle: 180,
    }),
  );
  expect(p.find("Spotify").props.rounded).toBe(16);
  p.unmount();
});

test("no artwork, no tint", async () => {
  app.running = false;
  const p = await start();
  expect(p.find("Spotify").props.bg_gradient).toBeUndefined();
  p.unmount();
});

test("pickTint finds the dominant colorful hue", () => {
  const red: [number, number, number] = [200, 30, 30];
  const blue: [number, number, number] = [30, 60, 200];
  const grey: [number, number, number] = [120, 120, 120];
  const tint = pickTint([...Array(60).fill(red), ...Array(25).fill(blue), ...Array(15).fill(grey)]);
  const hue = (hex: string) => {
    const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
    return r! > b! ? "red" : "blue";
  };
  expect(hue(tint.from)).toBe("red");
  expect(hue(tint.to)).toBe("blue");
});

test("pickTint mutes greyscale art", () => {
  const tint = pickTint(Array(100).fill([128, 128, 128]));
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(tint.from.slice(i, i + 2), 16));
  expect(Math.max(r!, g!, b!) - Math.min(r!, g!, b!)).toBeLessThan(10);
});

test("bmpPixels reads a top-down 24-bit BMP", () => {
  // 2×1 pixels: blue-green-red order, rows padded to 4 bytes.
  const header = new Uint8Array(54);
  const view = new DataView(header.buffer);
  view.setUint32(10, 54, true);
  view.setInt32(18, 2, true);
  view.setInt32(22, -1, true);
  view.setUint16(28, 24, true);
  const pixels = new Uint8Array([0, 0, 255, 255, 0, 0, 0, 0]);
  const bytes = new Uint8Array([...header, ...pixels]);
  expect(bmpPixels(bytes)).toEqual([
    [255, 0, 0],
    [0, 0, 255],
  ]);
});

test("a change Spotify announces shows without waiting for a poll", async () => {
  const p = await start();
  expect(p.find("Spotify").props.accessory).toBe("Paused");
  app.state = "playing";
  announce();
  await p.waitFor(() => expect(p.find("Spotify").props.accessory).toBe("Playing"));
  p.unmount();
});

test("the time counts on between reads while playing", async () => {
  app.state = "playing";
  const p = await start();
  await p.setCardOpen(true);
  expect(p.text()).toContain("1:05");
  setSystemTime(new Date(Date.now() + 10_000));
  try {
    await p.waitFor(() => expect(p.text()).toContain("1:15"), { timeout: 2000 });
  } finally {
    setSystemTime();
    p.unmount();
  }
});

test("asks for permission when macOS blocks it", async () => {
  app.denied = true;
  const p = await start();
  expect(p.text()).toContain("needs permission to control Spotify");
  expect(p.text()).not.toContain("isn't running");
  await p.press("Open Automation Settings");
  expect(commands.some((c) => c.includes("Privacy_Automation"))).toBe(true);
  app.denied = false;
  await p.press("Try Again");
  await p.waitFor(() => expect(p.text()).toContain("Song"));
  p.unmount();
});

test("controls take the artwork's accent color", async () => {
  const p = await start();
  await p.waitFor(() => expect(p.find("Play").props.bg).toBe("#e05a44"));
  expect(byId(p, "seek").props.color).toBe("#e05a44");
  expect(p.find("Copy Link").props.text_color).toBe("#e05a44");
  p.unmount();
});
