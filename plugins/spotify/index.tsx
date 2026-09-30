// Now playing in the Spotify desktop app, with playback controls. Spotify's
// playback notifications trigger a fresh read, the times count on locally in
// between, and a slow poll catches what Spotify doesn't announce, such as
// volume. macOS only: it drives the app with JavaScript for Automation, so
// there's nothing to sign in to.

import {
  Button,
  Card,
  Footer,
  Icon,
  Slider,
  Text,
  createStore,
  definePlugin,
  sidedoor,
  useCardOpen,
  useEffect,
  useInterval,
  useState,
  useStorage,
  type Color,
} from "@sidedoor/sdk";
import { tintFor, type Tint } from "./colors";
import * as spotify from "./spotify";
import type { Player } from "./spotify";

const SPOTIFY_GREEN = "#1db954";

/** A read of Spotify, and when it was taken, so the time can count on. */
type State = Player & { readAt: number };
type Running = Extract<State, { running: true }>;

/** `null` until the first read. Shared by the tile and the card. */
const player = createStore<State | null>(null);

/** Redraws the times while playing. */
const now = createStore(Date.now());

/** Seconds into the track, counted on from the last read while playing. */
function livePosition(p: Running, at = Date.now()) {
  if (p.state !== "playing") return p.position;
  // The clock can be older than the read; time never runs backwards.
  const position = p.position + Math.max(0, at - p.readAt) / 1000;
  return p.track ? Math.min(position, p.track.duration / 1000) : position;
}

/** Colors from the current album art, for the card's background. */
const tint = createStore<{ artwork: string; colors: Tint } | null>(null);

let tinting: string | null = null;
async function updateTint(artwork: string) {
  if (!artwork) return tint.set(null);
  if (tint.get()?.artwork === artwork || tinting === artwork) return;
  tinting = artwork;
  try {
    const colors = await tintFor(artwork);
    // A newer track may have started while this one was downloading.
    if (tinting === artwork) tint.set({ artwork, colors });
  } catch (error) {
    console.error("Couldn't read the artwork's colors:", error);
  } finally {
    if (tinting === artwork) tinting = null;
  }
}

let reading = false;
let readAgain = false;
async function refresh() {
  // A change that lands mid-read gets a read of its own afterwards.
  if (reading) return void (readAgain = true);
  reading = true;
  try {
    do {
      readAgain = false;
      const next = await spotify.read();
      player.set({ ...next, readAt: Date.now() });
      void updateTint(next.running ? (next.track?.artwork ?? "") : "");
    } while (readAgain);
  } catch (error) {
    console.error("Couldn't read Spotify:", error);
  } finally {
    reading = false;
  }
}

/** Spotify posts a pair of notifications per change; read once for both. */
let changeTimer: ReturnType<typeof setTimeout> | undefined;
function playbackChanged() {
  clearTimeout(changeTimer);
  changeTimer = setTimeout(refresh, 100);
}

/** Shows `optimistic` right away, runs the command, then reads the real state. */
async function run(command: () => Promise<unknown>, optimistic?: (p: Running) => Partial<Running>) {
  const current = player.get();
  if (optimistic && current?.running) {
    const base = { ...current, position: livePosition(current), readAt: Date.now() };
    player.set({ ...base, ...optimistic(base) });
  }
  try {
    await command();
  } catch (error) {
    console.error("Spotify command failed:", error);
  }
  await refresh();
}

const playPause = () =>
  run(spotify.playPause, (p) => ({ state: p.state === "playing" ? "paused" : "playing" }));
const next = () => run(spotify.next);
const previous = () => run(spotify.previous);
const open = () => run(spotify.open);

const SEEK_SECONDS = 15;

/** Jumps `delta` seconds, clamped to the episode. */
function seekBy(delta: number) {
  const current = player.get();
  if (!current?.running || !current.track) return;
  const end = current.track.duration / 1000;
  const position = Math.min(Math.max(0, livePosition(current) + delta), Math.max(0, end - 1));
  return run(() => spotify.seek(position), () => ({ position }));
}

/**
 * Sends volume changes one at a time while the slider drags, skipping to the
 * newest value, so a fast drag doesn't queue up a script per step.
 */
let volumeSending = false;
let volumeWanted: number | null = null;
async function sendVolume(volume: number) {
  volumeWanted = volume;
  if (volumeSending) return;
  volumeSending = true;
  try {
    while (volumeWanted !== null) {
      const next = volumeWanted;
      volumeWanted = null;
      await spotify.setVolume(next).catch((error) => console.error("Couldn't set volume:", error));
    }
  } finally {
    volumeSending = false;
  }
}

const isEpisode = (url: string) => url.startsWith("spotify:episode:");

const clock = (seconds: number) => {
  const s = Math.max(0, Math.floor(seconds));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
};

function Control(props: {
  id: string;
  label: string;
  icon: string;
  on_click: () => void;
  size?: number;
  color?: Color;
  primary?: boolean;
  /** The primary button's fill and icon colors. */
  accent?: Color;
  onAccent?: Color;
}) {
  const size = props.size ?? 32;
  return (
    <div
      id={props.id}
      label={props.label}
      size={size}
      rounded_full
      flex
      items_center
      justify_center
      cursor_pointer
      bg={props.primary ? (props.accent ?? SPOTIFY_GREEN) : "transparent"}
      hover={props.primary ? { opacity: 0.85 } : { bg: "fill" }}
      active={{ opacity: 0.7 }}
      on_click={props.on_click}
    >
      <Icon
        name={props.icon}
        icon_size={props.primary ? 18 : 16}
        color={props.primary ? (props.onAccent ?? "#000000") : (props.color ?? "label")}
      />
    </div>
  );
}

function Artwork({ src, size }: { src: string; size: number }) {
  return src ? (
    <img src={src} size={size} rounded={size / 8} object_fit="cover" flex_shrink_0 />
  ) : (
    <div size={size} rounded={size / 8} bg="fill" flex items_center justify_center flex_shrink_0>
      <Icon name="music" icon_size={size / 2.5} color="tertiary" />
    </div>
  );
}

export default definePlugin({
  name: "Spotify",
  icon: "music",
  width: 300,
  settings: {
    click: {
      title: "Clicking the dock item",
      type: "choice",
      options: ["Play/Pause", "Next Track", "Open Spotify"],
      default: "Play/Pause",
    },
  },

  onClick: () => {
    const click = sidedoor.settings().click;
    if (click === "Next Track") next();
    else if (click === "Open Spotify") open();
    else playPause();
  },

  actions: {
    playPause: { title: "Play/Pause", run: playPause },
    next: { title: "Next Track", run: next },
    previous: { title: "Previous Track", run: previous },
    back: { title: `Back ${SEEK_SECONDS} Seconds`, run: () => seekBy(-SEEK_SECONDS) },
    forward: { title: `Forward ${SEEK_SECONDS} Seconds`, run: () => seekBy(SEEK_SECONDS) },
    open: { title: "Open Spotify", run: open },
  },

  tile() {
    const p = player.use();
    const track = p?.running ? p.track : null;
    if (!track?.artwork) return <Icon name="music" icon_size={18} magnify />;
    return (
      <img
        src={track.artwork}
        size={30}
        rounded={6}
        object_fit="cover"
        opacity={p?.running && p.state === "playing" ? 1 : 0.55}
        transition={200}
        magnify
      />
    );
  },

  card() {
    const p = player.use();
    const colors = tint.use()?.colors;
    const cardOpen = useCardOpen();
    // The end time shows what's left; clicking it shows the length instead.
    const [showRemaining, setShowRemaining] = useStorage("showRemaining", true);
    // Where the sliders are while dragging (0–1), so a poll doesn't yank them back.
    const [seeking, setSeeking] = useState<number | null>(null);
    const [adjusting, setAdjusting] = useState<number | null>(null);
    const playing = p?.running === true && p.state === "playing";
    // The card always renders, so the watcher, poll and clock live here.
    useEffect(() => spotify.watch(playbackChanged), []);
    useEffect(() => void refresh(), [cardOpen]);
    useInterval(refresh, cardOpen ? 5000 : 30000);
    useInterval(() => now.set(Date.now()), cardOpen && playing ? 500 : null);
    const at = now.use();

    if (p === null) {
      return (
        <Card title="Spotify">
          <Text secondary>Loading…</Text>
        </Card>
      );
    }

    if (!p.running && p.denied) {
      return (
        <Card title="Spotify">
          <div flex flex_col items_center gap={10} py={8}>
            <Icon name="lock" icon_size={28} color="tertiary" />
            <Text secondary text_center>
              Sidedoor needs permission to control Spotify. Turn on Spotify under Sidedoor in
              Automation settings.
            </Text>
            <Button
              variant="primary"
              label="Open Automation Settings"
              on_click={spotify.openAutomationSettings}
            />
            <Button variant="link" label="Try Again" on_click={refresh} />
          </div>
        </Card>
      );
    }

    if (!p.running) {
      return (
        <Card title="Spotify">
          <div flex flex_col items_center gap={10} py={8}>
            <Icon name="music" icon_size={28} color="tertiary" />
            <Text secondary>Spotify isn't running.</Text>
            <Button variant="primary" icon="play" label="Open Spotify" on_click={open} />
          </div>
        </Card>
      );
    }

    const { track } = p;
    const duration = track ? track.duration / 1000 : 0;
    const position = seeking !== null ? seeking * duration : livePosition(p, at);
    // Controls take their color from the art, like the background.
    const accent = colors?.accent ?? SPOTIFY_GREEN;
    const progress = duration ? Math.min(1, position / duration) : 0;
    const volume = adjusting !== null ? Math.round(adjusting * 100) : p.volume;
    // Episodes swap shuffle and repeat, which do nothing there, for seeking.
    const podcast = track ? isEpisode(track.url) : false;

    return (
      <Card
        title="Spotify"
        accessory={playing ? "Playing" : p.state === "paused" ? "Paused" : "Stopped"}
        // Tints the card's frosted material without hiding it: the colors
        // are mostly transparent, and the radius matches the card's corners.
        rounded={16}
        bg_gradient={
          colors && { from: `${colors.from}73`, to: `${colors.to}2e`, angle: 180 }
        }
      >
        {track ? (
          <div flex items_center gap={12}>
            <Artwork src={track.artwork} size={64} />
            <div flex flex_col gap={2} min_w_0 flex_1>
              <Text variant="headline" truncate>{track.name}</Text>
              <Text secondary truncate>{track.artist}</Text>
              <Text variant="caption" tertiary truncate>{track.album}</Text>
            </div>
          </div>
        ) : (
          <div flex items_center gap={12}>
            <Artwork src="" size={64} />
            <Text secondary>Nothing playing</Text>
          </div>
        )}

        {track && (
          <div flex flex_col gap={4}>
            <Slider
              id="seek"
              w_full
              value={progress}
              color={accent}
              on_change={setSeeking}
              on_commit={(value) => {
                setSeeking(null);
                const position = value * duration;
                run(() => spotify.seek(position), () => ({ position }));
              }}
            />
            <div flex justify_between text_size={11} text_color="tertiary">
              <div>{clock(position)}</div>
              <div id="end-time" cursor_pointer on_click={() => setShowRemaining(!showRemaining)}>
                {showRemaining ? `-${clock(duration - position)}` : clock(duration)}
              </div>
            </div>
          </div>
        )}

        <div flex items_center justify_between px={4}>
          {podcast ? (
            <Control id="back" label={`Back ${SEEK_SECONDS} Seconds`} icon="rotate-ccw" on_click={() => seekBy(-SEEK_SECONDS)} />
          ) : (
            <Control
              id="shuffle"
              label="Shuffle"
              icon="shuffle"
              color={p.shuffling ? accent : "secondary"}
              on_click={() => run(() => spotify.setShuffling(!p.shuffling), () => ({ shuffling: !p.shuffling }))}
            />
          )}
          <Control id="previous" label="Previous Track" icon="skip-back" on_click={previous} />
          <Control
            id="play"
            label={playing ? "Pause" : "Play"}
            icon={playing ? "pause" : "play"}
            size={40}
            primary
            accent={accent}
            onAccent={colors?.onAccent}
            on_click={playPause}
          />
          <Control id="next" label="Next Track" icon="skip-forward" on_click={next} />
          {podcast ? (
            <Control id="forward" label={`Forward ${SEEK_SECONDS} Seconds`} icon="rotate-cw" on_click={() => seekBy(SEEK_SECONDS)} />
          ) : (
            <Control
              id="repeat"
              label="Repeat"
              icon="repeat"
              color={p.repeating ? accent : "secondary"}
              on_click={() => run(() => spotify.setRepeating(!p.repeating), () => ({ repeating: !p.repeating }))}
            />
          )}
        </div>

        <div flex items_center gap={8} px={2}>
          <Icon name={volume === 0 ? "volume-x" : "volume-1"} color="secondary" />
          <Slider
            id="volume"
            flex_1
            value={volume / 100}
            color="secondary"
            on_change={(value) => {
              setAdjusting(value);
              sendVolume(value * 100);
            }}
            on_commit={(value) => {
              setAdjusting(null);
              const volume = Math.round(value * 100);
              run(() => sendVolume(volume), () => ({ volume }));
            }}
          />
          <Icon name="volume-2" color="secondary" />
        </div>

        <Footer>
          <div flex justify_between w_full>
            <Button variant="link" label="Open Spotify" text_color={accent} on_click={open} />
            {track?.url && (
              <Button
                variant="link"
                label="Copy Link"
                text_color={accent}
                on_click={() => sidedoor.copy(spotify.webUrl(track.url))}
              />
            )}
          </div>
        </Footer>
      </Card>
    );
  },
});
