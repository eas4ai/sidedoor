# Example: Pomodoro

A complete plugin, copied from [`sdk/examples/plugins/pomodoro`](https://github.com/lassejlv/sidedoor/tree/main/sdk/examples/plugins/pomodoro). It shows:

- a custom tile
- a fitted card
- a `choice` setting
- `useStorage` and a shared `createStore` clock
- `onClick`, a context-menu action and a window
- a notification
- tests

## index.tsx

```tsx
// A focus timer: a native card with custom-styled pieces, and a dock tile
// that counts down. Clicking the tile starts and pauses it, the timer
// survives reloads and restarts, and a banner says when time is up.

import {
  Button,
  Card,
  Chart,
  Icon,
  Segmented,
  Text,
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

/** The last `count` days, oldest first, for the charts. */
function days(history: History, count: number) {
  return Array.from({ length: count }, (_, index) => {
    const day = new Date(Date.now() - (count - 1 - index) * 86_400_000);
    return {
      label:
        count <= 7
          ? day.toLocaleDateString("en", { weekday: "short" })
          : day.toLocaleDateString("en", { month: "short", day: "numeric" }),
      value: history[dayKey(day)] ?? 0,
    };
  });
}

const hours = (minutes: number) =>
  minutes < 60 ? `${minutes} min` : `${Math.floor(minutes / 60)} h ${minutes % 60} min`;

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
    history: { title: "Focus History…", run: () => sidedoor.openWindow("history") },
  },

  windows: {
    history: {
      title: "Focus History",
      width: 520,
      height: 400,
      render() {
        const [history] = useStorage<History>("history", {});
        const month = days(history, 30);
        const total = month.reduce((sum, day) => sum + day.value, 0);
        const best = month.reduce((top, day) => (day.value > top.value ? day : top), month[0]);
        return (
          <>
            <div flex gap={10}>
              {[
                ["Last 30 days", hours(total)],
                ["Daily average", hours(Math.round(total / 30))],
                ["Best day", best.value ? `${best.label}, ${hours(best.value)}` : "None yet"],
              ].map(([title, value]) => (
                <div key={title} flex_1 flex flex_col gap={2} p={10} rounded={10} bg="fill">
                  <Text variant="caption" secondary>{title}</Text>
                  <Text variant="headline">{value}</Text>
                </div>
              ))}
            </div>
            <Chart kind="area" h={200} color="orange" name="Minutes" y_axis grid data={month} />
            <Button
              variant="link"
              label="Clear History"
              on_click={() => sidedoor.storage.delete("history")}
            />
          </>
        );
      },
    },
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
        <Chart kind="bar" h={64} mt={4} color="orange" name="Minutes" data={days(history, 7)} />
        <Button variant="link" label="Show History" on_click={() => sidedoor.openWindow("history")} />
      </Card>
    );
  },
});
```

## index.test.tsx

```tsx
import { expect, setSystemTime, test } from "bun:test";
import { mount } from "@sidedoor/sdk/testing";
import pomodoro from "./index";

test("the tile starts and pauses the timer", async () => {
  const plugin = mount(pomodoro);
  expect(plugin.find("Pomodoro").props.accessory).toBe("Paused");
  await plugin.click();
  expect(plugin.find("Pomodoro").props.accessory).toBe("Focusing");
  await plugin.press("Pause");
  expect(plugin.find("Pomodoro").props.accessory).toBe("Paused");
  plugin.unmount();
});

test("a session that ends sends a banner and resets", async () => {
  setSystemTime(new Date("2026-09-29T09:00:00Z"));
  const plugin = mount(pomodoro, { storage: { timer: { minutes: 15, left: 900, endsAt: null } } });
  await plugin.press("Start");
  setSystemTime(new Date("2026-09-29T09:15:01Z"));
  await new Promise((resolve) => setTimeout(resolve, 1100));
  expect(plugin.notifications).toEqual([
    { title: "Time's up", body: "15 minutes of focus done. Take a break." },
  ]);
  expect(plugin.text()).toContain("15:00");
  const [chart] = plugin.findAll("Chart");
  const week = chart?.props.data as Array<{ value: number }>;
  expect(week.at(-1)?.value).toBe(15);
  plugin.unmount();
  setSystemTime();
});

test("Reset Timer in the menu puts the time back", async () => {
  const plugin = mount(pomodoro, { storage: { timer: { minutes: 25, left: 60, endsAt: null } } });
  expect(plugin.text()).toContain("1:00");
  await plugin.action("reset");
  expect(plugin.text()).toContain("25:00");
});

test("the history window adds up finished sessions", async () => {
  const today = new Date().toISOString().slice(0, 10);
  const plugin = mount(pomodoro, { storage: { history: { [today]: 75 } } });
  await plugin.action("history");
  await plugin.settle();
  expect(plugin.text("window:history")).toContain("1 h 15 min");
  await plugin.press("Clear History", "window:history");
  expect(plugin.text("window:history")).toContain("None yet");
});
```

## tsconfig.json

```json
{
  "compilerOptions": {
    "target": "ESNext",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "jsx": "react-jsx",
    "jsxImportSource": "@sidedoor/sdk",
    "strict": true,
    "noEmit": true,
    "skipLibCheck": true,
    "types": [
      "bun"
    ]
  }
}
```
