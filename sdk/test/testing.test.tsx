import { afterEach, expect, test } from "bun:test";
import {
  Button,
  Card,
  Icon,
  Input,
  Slider,
  Text,
  createStore,
  definePlugin,
  sidedoor,
  useEffect,
  useInterval,
  useState,
  useStorage,
} from "@sidedoor/sdk";
import { mount } from "@sidedoor/sdk/testing";
import { renderSurfaces, reset, restore, snapshot } from "../src/runtime";

afterEach(() => reset());

const counter = definePlugin({
  name: "Counter",
  settings: {
    step: { title: "Step", type: "number", default: 1 },
    unit: { title: "Unit", type: "choice", options: ["clicks", "taps"] },
  },
  card({ settings }) {
    const [count, setCount] = useStorage("count", 0);
    // Typed from the definition: a number and one of the options.
    const step: number = settings.step;
    const unit: "clicks" | "taps" = settings.unit;
    // @ts-expect-error: a typo is caught.
    void settings.stpe;
    return (
      <Card title="Counter" accessory={`${count} ${unit}`}>
        <Button label="Add" on_click={() => setCount(count + step)} />
        <Input id="note" on_submit={(text) => sidedoor.notify({ title: text })} />
      </Card>
    );
  },
  onClick: ({ settings }) => sidedoor.storage.set("count", settings.step * 100),
  actions: { reset: { title: "Reset", run: () => sidedoor.storage.set("count", 0) } },
});

test("mount renders, presses and reads back", async () => {
  const plugin = mount(counter, { settings: { step: 2 } });
  expect(plugin.find("Counter").props.accessory).toBe("0 clicks");
  await plugin.press("Add");
  await plugin.press("Add");
  expect(plugin.find("Counter").props.accessory).toBe("4 clicks");
  expect(plugin.storage.get<number>("count")).toBe(4);
});

test("mount clicks the tile, runs actions and follows settings", async () => {
  const plugin = mount(counter, { storage: { count: 7 } });
  expect(plugin.find("Counter").props.accessory).toBe("7 clicks");
  await plugin.click();
  expect(plugin.find("Counter").props.accessory).toBe("100 clicks");
  await plugin.action("reset");
  await plugin.setSettings({ unit: "taps" });
  expect(plugin.find("Counter").props.accessory).toBe("0 taps");
});

test("mount records notifications and inputs submit", async () => {
  const plugin = mount(counter);
  await plugin.find((element) => element.type === "Input").submit("Hello");
  expect(plugin.notifications).toEqual([{ title: "Hello", body: "" }]);
});

test("a plugin error fails the test that caused it", async () => {
  const broken = definePlugin({
    name: "Broken",
    card: () => (
      <Button
        label="Break"
        on_click={() => {
          throw new Error("nope");
        }}
      />
    ),
  });
  const plugin = mount(broken);
  await expect(plugin.press("Break")).rejects.toThrow("nope");
});

test("state survives a reload as a snapshot", () => {
  const Widget = () => {
    const [count] = useState(() => 0);
    const [when] = useState(() => new Date());
    return <div>{`${count} ${when instanceof Date}`}</div>;
  };
  const store = createStore({ open: false });
  renderSurfaces({ card: Widget });
  store.set({ open: true });
  // Pretend the count went up, as a click would.
  const saved = snapshot();
  const [path] = Object.keys(saved.instances);
  saved.instances[path].values[0] = 5;
  // Only plain JSON is kept: the Date is not.
  expect(saved.instances[path].values).toEqual({ 0: 5 });
  expect(saved.stores).toEqual({ 0: { open: true } });

  reset();
  restore(JSON.parse(JSON.stringify(saved)));
  const reloaded = createStore({ open: false });
  expect(reloaded.get()).toEqual({ open: true });
  expect(renderSurfaces({ card: Widget }).card).toEqual([{ t: "div", p: {}, c: ["5 true"] }]);
  // The snapshot is used once; later renders start fresh.
  reset();
  expect(renderSurfaces({ card: Widget }).card).toEqual([{ t: "div", p: {}, c: ["0 true"] }]);
});

test("windows render only while open", async () => {
  let renders = 0;
  const notes = definePlugin({
    name: "Notes",
    card: () => <Button label="More" on_click={() => sidedoor.openWindow("all")} />,
    windows: {
      all: {
        title: "All Notes",
        width: 520,
        render({ settings }) {
          renders++;
          const [count, setCount] = useState(0);
          return (
            <div>
              <Button label={`Seen ${count}`} on_click={() => setCount(count + 1)} />
              <Button label="Done" on_click={() => sidedoor.closeWindow("all")} />
              {Object.keys(settings).length}
            </div>
          );
        },
      },
    },
  });
  const plugin = mount(notes);
  expect(plugin.window("all")).toBeUndefined();
  expect(renders).toBe(0);

  await plugin.press("More");
  await plugin.settle();
  expect(plugin.find("Seen 0", "window:all").type).toBe("Button");
  await plugin.press("Seen 0", "window:all");
  expect(plugin.find("Seen 1", "window:all").type).toBe("Button");

  await plugin.press("Done", "window:all");
  await plugin.settle();
  expect(plugin.window("all")).toBeUndefined();

  // Reopened, it starts over: its state went with it.
  await plugin.press("More");
  await plugin.settle();
  expect(plugin.find("Seen 0", "window:all").type).toBe("Button");
  expect(plugin.text("window:all")).toBe("0");
  expect(plugin.sent.find((message) => message.type === "manifest")).toMatchObject({
    windows: [{ key: "all", title: "All Notes", width: 520, height: 360 }],
  });
});

test("icon-only controls are found and pressed by their label", async () => {
  const player = definePlugin({
    name: "Player",
    card() {
      const [playing, setPlaying] = useState(false);
      return (
        <div flex gap={8}>
          <div label={playing ? "Pause" : "Play"} on_click={() => setPlaying(!playing)}>
            <Icon name={playing ? "pause" : "play"} />
          </div>
        </div>
      );
    },
  });
  const plugin = mount(player);
  expect(plugin.find("Play").type).toBe("div");
  await plugin.press("Play");
  expect(plugin.find("Pause").children).toEqual([
    { t: "Icon", p: { name: "pause" }, c: [] },
  ]);
});

test("change drags a slider and commits where it was let go", async () => {
  const commits: number[] = [];
  const volume = definePlugin({
    name: "Volume",
    card() {
      const [level, setLevel] = useState(0.2);
      return (
        <div>
          <Slider id="volume" value={level} on_change={setLevel} on_commit={(v) => commits.push(v)} />
          <Text>{`${Math.round(level * 100)}%`}</Text>
        </div>
      );
    },
  });
  const plugin = mount(volume);
  const slider = plugin.find((element) => element.type === "Slider");
  expect(slider.props.value).toBe(0.2);
  await slider.change(0.75);
  expect(plugin.text()).toBe("75%");
  expect(plugin.find((element) => element.type === "Slider").props.value).toBe(0.75);
  expect(commits).toEqual([0.75]);
});

test("one settle waits out a chain of renders across ticks", async () => {
  const steps = createStore(0);
  const chain = definePlugin({
    name: "Chain",
    card() {
      const step = steps.use();
      // Each step lands a tick after the render before it, as a fetch would.
      useEffect(() => {
        if (step < 5) setTimeout(() => steps.set(step + 1), 0);
      }, [step]);
      return <Text>{`Step ${step}`}</Text>;
    },
  });
  const plugin = mount(chain);
  await plugin.settle();
  expect(plugin.text()).toBe("Step 5");
});

test("settle gives up on a plugin that never stops re-rendering", async () => {
  const busy = definePlugin({
    name: "Busy",
    card() {
      const [count, setCount] = useState(0);
      useInterval(() => setCount(count + 1), 0);
      return <Text>{count}</Text>;
    },
  });
  const plugin = mount(busy);
  await plugin.settle();
  expect(Number(plugin.text())).toBeGreaterThan(0);
  plugin.unmount();
});

test("waitFor retries until the check passes, or throws its last error", async () => {
  const slow = definePlugin({
    name: "Slow",
    card() {
      const [loaded, setLoaded] = useState(false);
      useEffect(() => {
        const timer = setTimeout(() => setLoaded(true), 40);
        return () => clearTimeout(timer);
      }, []);
      return <Text>{loaded ? "Loaded" : "Loading"}</Text>;
    },
  });
  const plugin = mount(slow);
  expect(plugin.text()).toBe("Loading");
  const text = await plugin.waitFor(() => {
    expect(plugin.text()).toBe("Loaded");
    return plugin.text();
  });
  expect(text).toBe("Loaded");
  await expect(
    plugin.waitFor(() => expect(plugin.text()).toBe("Never"), { timeout: 30 }),
  ).rejects.toThrow();
});
