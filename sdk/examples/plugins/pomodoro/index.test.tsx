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
  plugin.unmount();
  setSystemTime();
});

test("Reset Timer in the menu puts the time back", async () => {
  const plugin = mount(pomodoro, { storage: { timer: { minutes: 25, left: 60, endsAt: null } } });
  expect(plugin.text()).toContain("1:00");
  await plugin.action("reset");
  expect(plugin.text()).toContain("25:00");
});
