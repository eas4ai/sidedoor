import { afterEach, expect, test } from "bun:test";
import { Button, Card, Meter, useEffect, useState } from "@sidekick/sdk";
import { diff, dispatch, renderSurfaces, reset, runEffects, setInvalidateHandler } from "../src/runtime";

afterEach(() => {
  reset();
  setInvalidateHandler(() => {});
});

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

test("GPUI style props travel as they are written", () => {
  const Widget = () => (
    <div flex flex_col gap={8} px={12} text_color="secondary" hover={{ bg: "fill" }}>
      Hello {3}
    </div>
  );
  const { card } = renderSurfaces({ card: Widget });
  expect(card).toEqual([
    {
      t: "div",
      p: { flex: true, flex_col: true, gap: 8, px: 12, text_color: "secondary", hover: { bg: "fill" } },
      c: ["Hello ", "3"],
    },
  ]);
});

test("native components render as host elements with their props", () => {
  const Widget = () => (
    <Card title="System" rounded={20}>
      <Meter label="CPU" fraction={0.5} value="50%" color="blue" />
    </Card>
  );
  const [card] = renderSurfaces({ card: Widget }).card;
  expect(card).toMatchObject({
    t: "Card",
    p: { title: "System", rounded: 20 },
    c: [{ t: "Meter", p: { label: "CPU", fraction: 0.5 } }],
  });
});

test("handlers become stable references the host can call", async () => {
  let rendered = 0;
  const Counter = () => {
    const [count, setCount] = useState(0);
    rendered += 1;
    return <Button label={`Count ${count}`} on_click={() => setCount((n) => n + 1)} />;
  };
  let latest = renderSurfaces({ card: Counter }).card;
  setInvalidateHandler(() => {
    latest = renderSurfaces({ card: Counter }).card;
  });
  const handler = (latest[0] as any).p.on_click.$h;

  expect(dispatch(handler, undefined)).toBe(true);
  await tick();
  expect((latest[0] as any).p.label).toBe("Count 1");
  // The same key reaches the new handler after a re-render.
  expect((latest[0] as any).p.on_click.$h).toBe(handler);
  dispatch(handler, undefined);
  await tick();
  expect((latest[0] as any).p.label).toBe("Count 2");
  expect(rendered).toBe(3);
});

test("effects clean up when their component goes away", () => {
  const log: string[] = [];
  const Child = () => {
    useEffect(() => {
      log.push("mount");
      return () => log.push("unmount");
    }, []);
    return <div />;
  };
  let show = true;
  const Widget = () => <div>{show && <Child />}</div>;

  renderSurfaces({ card: Widget });
  runEffects();
  show = false;
  renderSurfaces({ card: Widget });
  runEffects();
  expect(log).toEqual(["mount", "unmount"]);
});

test("keyed children keep their own state when reordered", async () => {
  const Row = ({ label }: { label: string }) => {
    const [clicks, setClicks] = useState(0);
    return <div id={label} on_click={() => setClicks((n) => n + 1)}>{`${label}:${clicks}`}</div>;
  };
  let order = ["a", "b"];
  const Widget = () => <div>{order.map((label) => <Row key={label} label={label} />)}</div>;
  let tree = renderSurfaces({ card: Widget }).card;
  setInvalidateHandler(() => {
    tree = renderSurfaces({ card: Widget }).card;
  });
  type Branch = { c: Array<{ c: string[]; p: { on_click: { $h: string } } }> };
  dispatch((tree[0] as unknown as Branch).c[1].p.on_click.$h, undefined);
  await tick();
  order = ["b", "a"];
  tree = renderSurfaces({ card: Widget }).card;
  expect((tree[0] as unknown as Branch).c.map((row) => row.c[0])).toEqual(["b:1", "a:0"]);
});

test("re-renders become small patches", () => {
  const before = [{ t: "div", p: { gap: 4 }, c: ["a", { t: "div", p: { w: 10 }, c: [] }] }];
  const after = [{ t: "div", p: { gap: 4 }, c: ["b", { t: "div", p: { w: 20 }, c: [] }] }];
  expect(diff(before, after)).toEqual([
    { op: "replace", path: [0, 0], node: "b" },
    { op: "props", path: [0, 1], props: { w: 20 } },
  ]);
  expect(diff(before, before)).toEqual([]);
  // A different number of children replaces the parent.
  const grown = [{ t: "div", p: { gap: 4 }, c: ["a"] }];
  expect(diff(before, grown)).toEqual([{ op: "replace", path: [0], node: grown[0] }]);
  expect(diff(before, [...before, ...before])).toBeNull();
});
