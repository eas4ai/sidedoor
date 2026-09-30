// CPU, memory and startup disk, sampled every two seconds with Bun.

import {
  Card,
  Meter,
  NumberText,
  Sparkline,
  Title,
  createStore,
  definePlugin,
  useEffect,
} from "@sidedoor/sdk";
import {
  cpuTimes,
  formatBytes,
  formatMemory,
  percent,
  sample,
  type Snapshot,
} from "./sample";

/** Seconds between samples. */
export const INTERVAL = 2;
/** CPU readings kept for the sparkline. */
const HISTORY = 30;

const stats = createStore<{ snapshot: Snapshot | null; history: number[] }>({
  snapshot: null,
  history: [],
});

/** Samples while the plugin runs. The tile is always rendered, so it owns this. */
function useSampler() {
  useEffect(() => {
    let times = cpuTimes();
    let stopped = false;
    const tick = async () => {
      const next = await sample(times);
      if (stopped) return;
      times = next.times;
      stats.set(({ history }) => ({
        snapshot: next.snapshot,
        history: [...history, next.snapshot.cpu].slice(-HISTORY),
      }));
    };
    const timer = setInterval(tick, INTERVAL * 1000);
    // The first CPU reading needs a moment to compare against.
    const first = setTimeout(tick, 500);
    return () => {
      stopped = true;
      clearInterval(timer);
      clearTimeout(first);
    };
  }, []);
}

export default definePlugin({
  name: "Stats",
  icon: "cpu",
  width: 300,
  height: 206,
  tile: () => {
    useSampler();
    const { snapshot } = stats.use();
    return (
      <div flex flex_col items_center gap={2} magnify>
        {(
          [
            ["CPU", snapshot?.cpu],
            ["RAM", snapshot && percent(snapshot.memoryUsed, snapshot.memoryTotal)],
          ] as const
        ).map(([label, value]) => (
          <div key={label} flex flex_col items_center line_height={1.05}>
            <div text_size={8} font_weight="semibold" text_color="secondary" magnify>
              {label}
            </div>
            {value === undefined || value === null ? (
              <div text_size={11} font_weight="semibold" magnify>
                --
              </div>
            ) : (
              <NumberText
                id={`tile-${label}`}
                value={Math.round(value)}
                suffix="%"
                duration={700}
                text_size={11}
                font_weight="semibold"
                magnify
              />
            )}
          </div>
        ))}
      </div>
    );
  },
  card: () => {
    const { snapshot, history } = stats.use();
    if (!snapshot)
      return (
        <Card gap={4}>
          <Title>System</Title>
          <div text_size={12} text_color="secondary">
            {`Reading your ${process.platform === "win32" ? "PC" : "Mac"}…`}
          </div>
        </Card>
      );
    const cpu = Math.round(snapshot.cpu);
    return (
      <Card gap={10}>
        <div flex items_center justify_between>
          <Title>System</Title>
          <Sparkline values={history.map((value) => value / 100)} />
        </div>
        <Meter
          id="cpu"
          icon="cpu"
          label="CPU"
          fraction={snapshot.cpu / 100}
          animated
          value_number={cpu}
          value_suffix="%"
          color="blue"
        />
        <Meter
          id="memory"
          icon="memory-stick"
          label="Memory"
          fraction={percent(snapshot.memoryUsed, snapshot.memoryTotal) / 100}
          animated
          value={`${formatMemory(snapshot.memoryUsed)} of ${formatMemory(snapshot.memoryTotal)}`}
          color="green"
        />
        <Meter
          id="storage"
          icon="hard-drive"
          label="Storage"
          fraction={percent(snapshot.diskUsed, snapshot.diskTotal) / 100}
          animated
          value={`${formatBytes(snapshot.diskTotal - snapshot.diskUsed)} free`}
          color="orange"
        />
        <div text_size={10} text_color="tertiary">
          Refreshes every {INTERVAL} s
        </div>
      </Card>
    );
  },
});
