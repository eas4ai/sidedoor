// A focus timer: a native card with custom-styled pieces, and a dock tile
// that counts down.

import {
  Button,
  Card,
  Icon,
  Segmented,
  createStore,
  definePlugin,
  sidekick,
  useEffect,
  useInterval,
  useSetting,
} from "@sidekick/sdk";

const LENGTHS = [15, 25, 50];

const initial = Number(sidekick.settings().length ?? 25);
const timer = createStore({ minutes: initial, left: initial * 60, running: false });

const clock = (seconds: number) =>
  `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;

function restart(minutes: number) {
  timer.set({ minutes, left: minutes * 60, running: false });
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

  tile() {
    const { left, running } = timer.use();
    return (
      <div flex flex_col items_center gap={2}>
        <Icon name={running ? "timer" : "timer-off"} icon_size={17} color={running ? "orange" : "label"} />
        <div text_size={11} font_weight="semibold">
          {`${Math.ceil(left / 60)}m`}
        </div>
      </div>
    );
  },

  card() {
    const { minutes, left, running } = timer.use();
    // A new default from Settings › Plugins applies while the timer is idle.
    const length = Number(useSetting<string>("length") ?? 25);
    useEffect(() => {
      if (!timer.get().running) restart(length);
    }, [length]);
    // The card always renders, so the clock ticks here.
    useInterval(
      () =>
        timer.set((t) =>
          t.left <= 1 ? { ...t, left: t.minutes * 60, running: false } : { ...t, left: t.left - 1 },
        ),
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
            on_click={() => timer.set((t) => ({ ...t, running: !t.running }))}
          />
          <Button icon="rotate-ccw" label="Reset" on_click={() => restart(minutes)} />
        </div>
      </Card>
    );
  },
});
