// Talks to the Spotify desktop app through JavaScript for Automation, so
// there's no login or API key. `Application("Spotify").running()` doesn't
// launch the app, and a missing app throws rather than asking where it is.

import { readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

export interface Track {
  name: string;
  artist: string;
  album: string;
  /** Milliseconds. */
  duration: number;
  artwork: string;
  /** `spotify:track:…` */
  url: string;
}

export type Player =
  /** `denied`: macOS automation permission for Spotify is off. */
  | { running: false; denied?: boolean }
  | {
      running: true;
      state: "playing" | "paused" | "stopped";
      track: Track | null;
      /** Seconds. */
      position: number;
      /** 0–100. */
      volume: number;
      shuffling: boolean;
      repeating: boolean;
    };

/** Runs a JXA script and returns what it printed. Tests replace it. */
export type Runner = (script: string) => Promise<string>;

let runner: Runner = async (script) => {
  const proc = Bun.spawn(["osascript", "-l", "JavaScript", "-e", script], {
    stdout: "pipe",
    stderr: "pipe",
  });
  const [out, err, code] = await Promise.all([
    new Response(proc.stdout).text(),
    new Response(proc.stderr).text(),
    proc.exited,
  ]);
  if (code !== 0) throw new Error(err.trim() || `osascript exited with ${code}`);
  return out.trim();
};

export const setRunner = (next: Runner) => {
  runner = next;
};

const READ = `
let out;
try {
  const app = Application("Spotify");
  if (!app.running()) out = { running: false };
  else {
    const state = app.playerState();
    let track = null;
    if (state !== "stopped") {
      const t = app.currentTrack;
      track = { name: t.name(), artist: t.artist(), album: t.album(), duration: t.duration(),
                artwork: t.artworkUrl() || "", url: t.spotifyUrl() || "" };
    }
    out = { running: true, state, track, position: app.playerPosition() || 0,
            volume: app.soundVolume(), shuffling: app.shuffling(), repeating: app.repeating() };
  }
} catch (e) { out = { running: false, denied: /-1743|not authori[sz]ed/i.test(String(e)) }; }
JSON.stringify(out);`;

export async function read(): Promise<Player> {
  return JSON.parse(await runner(READ)) as Player;
}

/** Runs `body` with `app` bound to Spotify, only if it's already running. */
const command = (body: string) =>
  runner(`const app = Application("Spotify"); if (app.running()) { ${body} } ""`);

export const playPause = () => command("app.playpause();");
export const next = () => command("app.nextTrack();");
export const previous = () => command("app.previousTrack();");
export const setVolume = (volume: number) =>
  command(`app.soundVolume = ${Math.round(Math.min(100, Math.max(0, volume)))};`);
export const setShuffling = (on: boolean) => command(`app.shuffling = ${on};`);
export const setRepeating = (on: boolean) => command(`app.repeating = ${on};`);
export const seek = (seconds: number) => command(`app.playerPosition = ${Math.max(0, seconds)};`);
export const open = () => runner(`Application("Spotify").activate(); ""`);

/** Opens System Settings at Privacy & Security › Automation. */
export const openAutomationSettings = () =>
  runner(`const app = Application.currentApplication(); app.includeStandardAdditions = true;
app.openLocation("x-apple.systempreferences:com.apple.preference.security?Privacy_Automation"); ""`);

/** `spotify:track:ID` → `https://open.spotify.com/track/ID`. */
export const webUrl = (uri: string) => {
  const [, kind, id] = uri.split(":");
  return kind && id ? `https://open.spotify.com/${kind}/${id}` : "";
};

// MARK: Watching

// Spotify posts `com.spotify.client.PlaybackStateChanged` to the distributed
// notification center on play, pause, seek and track changes. A long-running
// JXA script listens and prints a line for each, so the plugin reads the new
// state at once instead of polling. The script quits itself after a few
// minutes and is started again, so one orphaned by a plugin reload can't
// linger; the last one's pid is also kept so a reload can stop it at once.

const WATCH = `
ObjC.import("Foundation");
ObjC.registerSubclass({
  name: "SidedoorSpotifyObserver",
  methods: { "changed:": { types: ["void", ["id"]], implementation: function () { console.log("changed"); } } },
});
const observer = $.SidedoorSpotifyObserver.alloc.init;
$.NSDistributedNotificationCenter.defaultCenter.addObserverSelectorNameObject(
  observer, "changed:", "com.spotify.client.PlaybackStateChanged", $());
const end = Date.now() + 180000;
while (Date.now() < end) $.NSRunLoop.currentRunLoop.runUntilDate($.NSDate.dateWithTimeIntervalSinceNow(1));`;

const PID_FILE = join(tmpdir(), "sidedoor-spotify-watch.pid");

/** Stops a watcher left behind by an earlier run of the plugin. */
function stopStale() {
  try {
    const pid = Number(readFileSync(PID_FILE, "utf8"));
    if (!pid) return;
    const command = Bun.spawnSync(["ps", "-p", String(pid), "-o", "command="]).stdout.toString();
    if (command.includes("osascript") && command.includes("JavaScript")) process.kill(pid);
  } catch {
    // No file, or the process is already gone.
  }
}

/** Calls `onChange` whenever Spotify's playback changes. Returns a stop function. */
export type Watcher = (onChange: () => void) => () => void;

let watcher: Watcher = (onChange) => {
  let stopped = false;
  let proc: ReturnType<typeof Bun.spawn> | null = null;
  let failures = 0;
  stopStale();

  const start = async () => {
    const startedAt = Date.now();
    proc = Bun.spawn(["osascript", "-l", "JavaScript", "-e", WATCH], {
      stdout: "ignore",
      stderr: "pipe",
    });
    await Bun.write(PID_FILE, String(proc.pid));
    // `console.log` in osascript writes to stderr.
    const decoder = new TextDecoder();
    for await (const chunk of proc.stderr as ReadableStream<Uint8Array>) {
      if (decoder.decode(chunk).includes("changed")) onChange();
    }
    const code = await proc.exited;
    if (stopped) return;
    // A script that dies at once, again and again, is broken; stop retrying
    // and leave it to the plugin's slower poll.
    failures = Date.now() - startedAt < 5000 ? failures + 1 : 0;
    if (failures >= 3) return console.error(`Spotify watcher stopped (exit ${code}).`);
    setTimeout(start, 1000);
  };
  void start();

  return () => {
    stopped = true;
    proc?.kill();
  };
};

export const setWatcher = (next: Watcher) => {
  watcher = next;
};

export const watch = (onChange: () => void) => watcher(onChange);
