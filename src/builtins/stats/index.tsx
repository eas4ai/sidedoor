/** @jsxImportSource ../../../sdk/src */
import {
  Card,
  NumberText,
  Meter,
  Sparkline,
  Title,
  definePlugin,
  useData,
} from "../../../sdk/src";

export default definePlugin({
  name: "Stats",
  icon: "cpu",
  width: 300,
  height: 206,
  data: ["stats"],
  tile: () => {
    const stats = useData("stats");
    return (
      <div flex flex_col items_center gap={2} magnify>
        {(
          [
            ["CPU", stats?.cpu],
            ["RAM", stats?.memoryPercent],
          ] as const
        ).map(([label, value]) => (
          <div key={label} flex flex_col items_center line_height={1.05}>
            <div
              text_size={8}
              font_weight="semibold"
              text_color="secondary"
              magnify
            >
              {label}
            </div>
            {value === undefined ? (
              <div text_size={11} font_weight="semibold" magnify>
                --
              </div>
            ) : (
              <NumberText
                id={`tile-${label}`}
                value={value}
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
    const stats = useData("stats");
    if (!stats)
      return (
        <Card gap={4}>
          <Title>System</Title>
          <div text_size={12} text_color="secondary">
            {`Reading your ${process.platform === "win32" ? "PC" : "Mac"}…`}
          </div>
        </Card>
      );
    return (
      <Card gap={10}>
        <div flex items_center justify_between>
          <Title>System</Title>
          <Sparkline values={stats.history.map((value) => value / 100)} />
        </div>
        <Meter
          id="cpu"
          icon="cpu"
          label="CPU"
          fraction={stats.cpu / 100}
          animated
          value_number={stats.cpu}
          value_suffix="%"
          color="blue"
        />
        <Meter
          id="memory"
          icon="memory-stick"
          label="Memory"
          fraction={stats.memoryPercent / 100}
          animated
          value={`${stats.memoryUsed} of ${stats.memoryTotal}`}
          color="green"
        />
        <Meter
          id="storage"
          icon="hard-drive"
          label="Storage"
          fraction={stats.diskPercent / 100}
          animated
          value={`${stats.diskFree} free`}
          color="orange"
        />
        <div text_size={10} text_color="tertiary">
          Refreshes every {stats.interval} s
        </div>
      </Card>
    );
  },
});
