// Current conditions and the next hours from Open-Meteo, which needs no key.

export type Condition =
  | "clear"
  | "partly_cloudy"
  | "cloudy"
  | "fog"
  | "drizzle"
  | "rain"
  | "snow"
  | "thunderstorm";

export interface Place {
  name: string;
  latitude: number;
  longitude: number;
  /** Region and country, where they differ from the name. */
  detail: string;
}

export interface Hour {
  hour: number;
  temperature: number;
  condition: Condition;
}

export interface Weather {
  temperature: number;
  high: number;
  low: number;
  condition: Condition;
  isDay: boolean;
  hours: Hour[];
}

export type Unit = "Celsius" | "Fahrenheit";

/** A WMO weather code, as Open-Meteo reports it. */
export function condition(code: number): Condition {
  if (code === 0) return "clear";
  if (code === 1 || code === 2) return "partly_cloudy";
  if (code === 45 || code === 48) return "fog";
  if (code >= 51 && code <= 57) return "drizzle";
  if ((code >= 61 && code <= 67) || (code >= 80 && code <= 82)) return "rain";
  if ((code >= 71 && code <= 77) || code === 85 || code === 86) return "snow";
  if (code >= 95 && code <= 99) return "thunderstorm";
  return "cloudy";
}

export const LABELS: Record<Condition, string> = {
  clear: "Clear",
  partly_cloudy: "Partly Cloudy",
  cloudy: "Cloudy",
  fog: "Fog",
  drizzle: "Drizzle",
  rain: "Rain",
  snow: "Snow",
  thunderstorm: "Thunderstorms",
};

async function get(url: string): Promise<unknown> {
  let response: Response;
  try {
    response = await fetch(url, { signal: AbortSignal.timeout(15_000) });
  } catch (error) {
    throw new Error(`Couldn't reach Open-Meteo (${(error as Error).message})`);
  }
  if (!response.ok) throw new Error(`Open-Meteo answered ${response.status}`);
  return response.json();
}

/** The best match for `query`, or `null` if there's none. */
export async function findPlace(query: string): Promise<Place | null> {
  const url = `https://geocoding-api.open-meteo.com/v1/search?name=${encodeURIComponent(query.trim())}&count=1&format=json`;
  return parsePlace(await get(url));
}

export function parsePlace(json: unknown): Place | null {
  const result = (json as { results?: Array<Record<string, unknown>> }).results?.[0];
  if (!result) return null;
  const name = String(result.name);
  return {
    name,
    latitude: Number(result.latitude),
    longitude: Number(result.longitude),
    detail: [result.admin1, result.country]
      .filter((part): part is string => typeof part === "string" && part !== name)
      .join(", "),
  };
}

export async function forecast(place: Place, unit: Unit): Promise<Weather> {
  const url =
    `https://api.open-meteo.com/v1/forecast?latitude=${place.latitude}&longitude=${place.longitude}` +
    "&current=temperature_2m,weather_code,is_day" +
    "&hourly=temperature_2m,weather_code&forecast_hours=6" +
    "&daily=temperature_2m_max,temperature_2m_min&forecast_days=1&timezone=auto" +
    (unit === "Fahrenheit" ? "&temperature_unit=fahrenheit" : "");
  return parseForecast(await get(url));
}

export function parseForecast(json: unknown): Weather {
  const data = json as {
    current?: { temperature_2m: number; weather_code: number; is_day: number };
    hourly?: { time: string[]; temperature_2m: number[]; weather_code: number[] };
    daily?: { temperature_2m_max: number[]; temperature_2m_min: number[] };
  };
  const high = data.daily?.temperature_2m_max?.[0];
  const low = data.daily?.temperature_2m_min?.[0];
  if (!data.current || !data.hourly || high === undefined || low === undefined)
    throw new Error("Open-Meteo sent an incomplete forecast");
  const { time, temperature_2m, weather_code } = data.hourly;
  return {
    temperature: data.current.temperature_2m,
    high,
    low,
    condition: condition(data.current.weather_code),
    isDay: data.current.is_day === 1,
    // Times look like "2026-09-28T22:00".
    hours: time.map((stamp, index) => ({
      hour: Number(stamp.slice(11, 13)),
      temperature: temperature_2m[index] ?? 0,
      condition: condition(weather_code[index] ?? 3),
    })),
  };
}

/** Rounded as people read temperatures, halves away from zero. */
export const degrees = (value: number) => `${Math.sign(value) * Math.round(Math.abs(value))}°`;
