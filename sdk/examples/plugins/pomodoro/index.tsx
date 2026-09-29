// A focus timer: a native card with custom-styled pieces, and a dock tile
// that counts down. Clicking the tile starts and pauses it, the timer
// survives reloads and restarts, and a banner says when time is up.

import {
  Button,
  Card,
  Chart,
  Icon,
  Segmented,
  createStore,
  definePlugin,
  sidedoor,
  useEffect,
  useInterval,
  useRef,
  useStorage,
} from "@sidedoor/sdk";

const LENGTHS = [15, 25, 50];

/** Saved as it changes. While running, `endsAt` is when time is up. */
interface Timer {
  minutes: number;
  left: number;
  endsAt: number | null;
}

const fresh = (minutes: number): Timer => ({ minutes, left: minutes * 60, endsAt: null });
const initial = fresh(Number(sidedoor.settings().length ?? 25));
const saved = () => sidedoor.storage.get<Timer>("timer") ?? initial;
const save = (timer: Timer) => sidedoor.storage.set("timer", timer);

/** Seconds left, counted from the clock so a restart doesn't lose time. */
const secondsLeft = (timer: Timer) =>
  timer.endsAt === null ? timer.left : Math.max(0, Math.ceil((timer.endsAt - Date.now()) / 1000));

const clock = (seconds: number) =>
  `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;

/** Redraws once a second while the timer runs. */
const now = createStore(Date.now());

const restart = (minutes: number) => save(fresh(minutes));

/** Minutes of finished sessions, by day (`YYYY-MM-DD`). */
type History = Record<string, number>;

const dayKey = (date: Date) => date.toISOString().slice(0, 10);

function logSession(minutes: number) {
  const history = sidedoor.storage.get<History>("history") ?? {};
  const today = dayKey(new Date());
  sidedoor.storage.set("history", { ...history, [today]: (history[today] ?? 0) + minutes });
}

/** The last seven days, oldest first, for the chart. */
function week(history: History) {
  return Array.from({ length: 7 }, (_, index) => {
    const day = new Date(Date.now() - (6 - index) * 86_400_000);
    return {
      label: day.toLocaleDateString("en", { weekday: "short" }),
      value: history[dayKey(day)] ?? 0,
    };
  });
}

function toggle() {
  const timer = saved();
  save(
    timer.endsAt === null
      ? { ...timer, endsAt: Date.now() + timer.left * 1000 }
      : { ...timer, left: secondsLeft(timer), endsAt: null },
  );
}

export default definePlugin({
  name: "Pomodoro",
  icon: "timer",
  width: 290,
  settings: {
    length: {
      title: "Default length",
      description: "Minutes a new session starts with.",
      type: "choice",
      options: ["15", "25", "50"],
      default: "25",
    },
  },

  onClick: toggle,
  actions: {
    reset: { title: "Reset Timer", run: () => restart(saved().minutes) },
  },

  tile() {
    const [timer] = useStorage("timer", initial);
    now.use();
    const left = secondsLeft(timer);
    const running = timer.endsAt !== null;
    return (
      <div flex flex_col items_center gap={2}>
        <Icon name={running ? "timer" : "timer-off"} icon_size={17} color={running ? "orange" : "label"} />
        <div text_size={11} font_weight="semibold">
          {`${Math.ceil(left / 60)}m`}
        </div>
      </div>
    );
  },

  card({ settings }) {
    const [timer] = useStorage("timer", initial);
    const [history] = useStorage<History>("history", {});
    now.use();
    const { minutes } = timer;
    const left = secondsLeft(timer);
    const running = timer.endsAt !== null;
    // A new default from Settings › Plugins applies while the timer is idle.
    const length = Number(settings.length);
    const lastLength = useRef(length);
    useEffect(() => {
      if (lastLength.current === length) return;
      lastLength.current = length;
      if (saved().endsAt === null) restart(length);
    }, [length]);
    // The card always renders, so the clock ticks here.
    useInterval(
      () => {
        now.set(Date.now());
        const current = saved();
        if (current.endsAt !== null && secondsLeft(current) === 0) {
          restart(current.minutes);
          logSession(current.minutes);
          sidedoor.notify({
            title: "Time's up",
            body: `${current.minutes} minutes of focus done. Take a break.`,
          });
        }
      },
      running ? 1000 : null,
    );
    const progress = 1 - left / (minutes * 60);

    return (
      <Card title="Pomodoro" accessory={running ? "Focusing" : "Paused"}>
        <div flex justify_center>
          <div text_size={40} font_weight="semibold" text_color={running ? "label" : "secondary"}>
            {clock(left)}
          </div>
        </div>
        <div h={6} w_full rounded_full bg="track">
          <div h_full rounded_full bg="orange" w={`${Math.round(progress * 100)}%`} transition={900} />
        </div>
        <Segmented
          w_full
          mt={2}
          options={LENGTHS.map((length) => `${length} min`)}
          selected={LENGTHS.indexOf(minutes)}
          on_change={(index) => restart(LENGTHS[index])}
        />
        <div flex gap={8}>
          <Button
            flex_1
            variant="primary"
            icon={running ? "pause" : "play"}
            label={running ? "Pause" : "Start"}
            on_click={toggle}
          />
          <Button icon="rotate-ccw" label="Reset" on_click={() => restart(minutes)} />
        </div>
        <Chart kind="bar" h={64} mt={4} color="orange" name="Minutes" data={week(history)} />
      </Card>
    );
  },
});
