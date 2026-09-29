export type DataSource = keyof NativeData;

export type WeatherCondition =
  | "clear"
  | "partly_cloudy"
  | "cloudy"
  | "fog"
  | "drizzle"
  | "rain"
  | "snow"
  | "thunderstorm";
export interface WeatherReading {
  temperature: number;
  high: number;
  low: number;
  condition: WeatherCondition;
  is_day: boolean;
  hours: { hour: number; temperature: number; condition: WeatherCondition }[];
}
export type WeatherData = {
  location: { name: string; latitude: number; longitude: number };
} & (
  | { status: "loading" }
  | { status: "failed"; message: string }
  | {
      status: "ready";
      weather: WeatherReading;
      conditionLabel: string;
      updatedMinutes: number;
    }
);
export interface StatsData {
  cpu: number;
  memoryPercent: number;
  diskPercent: number;
  memoryUsed: string;
  memoryTotal: string;
  diskFree: string;
  history: number[];
  interval: number;
}
export interface ClipboardEntry {
  id: number;
  title: string;
  age: string;
  source: string | null;
  kind:
    | { type: "text"; text: string }
    | { type: "link"; url: string }
    | { type: "image"; path: string; width: number; height: number }
    | { type: "file"; path: string };
}
export interface ClipboardData {
  count: number;
  /** The five most recent copies; the native History window holds the full list. */
  entries: ClipboardEntry[];
  clearArmed: boolean;
}
export interface NativeData {
  weather: WeatherData;
  stats: StatsData | null;
  clipboard: ClipboardData;
}
