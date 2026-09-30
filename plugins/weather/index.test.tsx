import { afterEach, expect, spyOn, test } from "bun:test";
import { mount } from "@sidedoor/sdk/testing";
import weatherPlugin from "./index";
import { condition, degrees, parseForecast, parsePlace } from "./forecast";

const PLACES = {
  results: [
    { name: "Aalborg", latitude: 57.048, longitude: 9.9187, admin1: "North Denmark", country: "Denmark" },
  ],
};
const FORECAST = {
  current: { temperature_2m: 13.2, weather_code: 3, is_day: 0 },
  hourly: {
    time: ["2026-09-28T22:00", "2026-09-28T23:00"],
    temperature_2m: [12.8, 12.1],
    weather_code: [3, 61],
  },
  daily: { temperature_2m_max: [16.0], temperature_2m_min: [11.4] },
};

afterEach(() => {
  (globalThis.fetch as unknown as { mockRestore?: () => void }).mockRestore?.();
});

test("Open-Meteo's codes and forecasts read as the card shows them", () => {
  const weather = parseForecast(FORECAST);
  expect(weather.condition).toBe("cloudy");
  expect(weather.isDay).toBe(false);
  expect(weather.high).toBe(16);
  expect(weather.hours.map((hour) => [hour.hour, hour.condition])).toEqual([
    [22, "cloudy"],
    [23, "rain"],
  ]);
  expect(() => parseForecast({})).toThrow();
  expect(condition(95)).toBe("thunderstorm");
  expect(degrees(-2.5)).toBe("-3°");
  expect(degrees(12.5)).toBe("13°");
});

test("places are described by region and country", () => {
  expect(parsePlace(PLACES)?.detail).toBe("North Denmark, Denmark");
  expect(parsePlace({ generationtime_ms: 0.1 })).toBeNull();
});

test("the card shows the forecast for the city in settings", async () => {
  const urls: string[] = [];
  spyOn(globalThis, "fetch").mockImplementation((async (input: string | URL | Request) => {
    const url = String(input);
    urls.push(url);
    return Response.json(url.includes("geocoding") ? PLACES : FORECAST);
  }) as typeof fetch);
  const plugin = mount(weatherPlugin, { settings: { city: "Aalborg", unit: "Fahrenheit" } });
  await plugin.waitFor(() => expect(plugin.text("card")).toContain("High 16°, low 11°"));
  expect(plugin.text("card")).toContain("Aalborg");
  expect(plugin.text("card")).toContain("Cloudy");
  expect(urls[0]).toContain("name=Aalborg");
  expect(urls[1]).toContain("temperature_unit=fahrenheit");
  plugin.unmount();
});

test("a city that can't be found says so", async () => {
  spyOn(globalThis, "fetch").mockImplementation((async () => Response.json({})) as unknown as typeof fetch);
  const plugin = mount(weatherPlugin, { settings: { city: "Nowhereville", unit: "Celsius" } });
  await plugin.waitFor(() => expect(plugin.text("card")).toContain("Couldn't find “Nowhereville”"));
  plugin.unmount();
});
