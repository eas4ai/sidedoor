// The weather where you choose, from Open-Meteo, refreshed every 20 minutes.

import {
  Card,
  Footer,
  Icon,
  Title,
  createStore,
  definePlugin,
  sidedoor,
  useEffect,
  useInterval,
  useState,
} from "@sidedoor/sdk";
import {
  LABELS,
  degrees,
  findPlace,
  forecast,
  type Condition,
  type Place,
  type Unit,
  type Weather,
} from "./forecast";

const REFRESH_MINUTES = 20;

type State =
  | { status: "loading"; place: Place | null }
  | { status: "failed"; place: Place | null; message: string }
  | { status: "ready"; place: Place; weather: Weather; updatedAt: number };

export const weather = createStore<State>({ status: "loading", place: null });

function glyph(condition: Condition, day: boolean): string {
  switch (condition) {
    case "clear":
      return day ? "sun" : "moon";
    case "partly_cloudy":
      return day ? "cloud-sun" : "cloud-moon";
    case "cloudy":
      return "cloud";
    case "fog":
      return "cloud-fog";
    case "drizzle":
      return "cloud-drizzle";
    case "rain":
      return "cloud-rain";
    case "snow":
      return "cloud-snow";
    case "thunderstorm":
      return "cloud-lightning";
  }
}

/** Finds the city once, keeping the answer, since it rarely changes. */
async function placeFor(city: string): Promise<Place> {
  const saved = sidedoor.storage.get<{ query: string; place: Place }>("place");
  if (saved?.query === city) return saved.place;
  const place = await findPlace(city);
  if (!place) throw new Error(`Couldn't find “${city}”. Check the city in Settings › Plugins.`);
  sidedoor.storage.set("place", { query: city, place });
  return place;
}

async function refresh(city: string, unit: Unit) {
  const previous = weather.get();
  try {
    const place = await placeFor(city);
    weather.set({ status: "ready", place, weather: await forecast(place, unit), updatedAt: Date.now() });
  } catch (error) {
    // A failed refresh keeps showing the last forecast.
    if (previous.status !== "ready")
      weather.set({ status: "failed", place: previous.place, message: (error as Error).message });
  }
}

export default definePlugin({
  name: "Weather",
  icon: "cloud",
  width: 300,
  height: 190,
  settings: {
    city: {
      title: "City",
      description: "Where the forecast is for.",
      type: "text",
      default: "Aalborg",
    },
    unit: {
      title: "Temperature",
      type: "choice",
      options: ["Celsius", "Fahrenheit"],
      default: "Celsius",
    },
  },
  // The tile is always rendered, so it keeps the forecast fresh.
  tile: ({ settings }) => {
    const city = settings.city.trim() || "Aalborg";
    useEffect(() => {
      weather.set({ status: "loading", place: null });
      void refresh(city, settings.unit);
      const timer = setInterval(() => void refresh(city, settings.unit), REFRESH_MINUTES * 60_000);
      return () => clearInterval(timer);
    }, [city, settings.unit]);
    const state = weather.use();
    const ready = state.status === "ready" ? state.weather : null;
    return (
      <div flex flex_col items_center gap={3} magnify>
        <Icon
          name={
            ready
              ? glyph(ready.condition, ready.isDay)
              : state.status === "failed"
                ? "cloud-off"
                : "cloud"
          }
          icon_size={17}
          magnify
        />
        <div text_size={12} font_weight="semibold" magnify>
          {ready ? degrees(ready.temperature) : "--°"}
        </div>
      </div>
    );
  },
  card: ({ settings }) => {
    const state = weather.use();
    // Keeps "Updated … min ago" current while the card is open.
    const [, setNow] = useState(Date.now());
    useInterval(() => setNow(Date.now()), 30_000);
    const location = state.place?.name ?? (settings.city.trim() || "Weather");
    if (state.status !== "ready")
      return (
        <Card gap={4}>
          <Title>{location}</Title>
          <div text_size={12} text_color="secondary">
            {state.status === "failed" ? state.message : "Loading weather…"}
          </div>
        </Card>
      );
    const { weather: now, updatedAt } = state;
    const minutes = Math.floor((Date.now() - updatedAt) / 60_000);
    return (
      <Card gap={0}>
        <div flex items_start justify_between>
          <div flex flex_col>
            <Title>{location}</Title>
            <div text_size={12} text_color="secondary">
              {LABELS[now.condition]}
            </div>
          </div>
          <div flex items_center gap={8}>
            <Icon name={glyph(now.condition, now.isDay)} icon_size={24} />
            <div text_size={28} font_weight={300}>
              {degrees(now.temperature)}
            </div>
          </div>
        </div>
        <div mt={2} text_size={12} text_color="secondary">
          High {degrees(now.high)}, low {degrees(now.low)}
        </div>
        <div mt={10} flex justify_between>
          {now.hours.map((hour) => (
            <div key={hour.hour} flex flex_col items_center gap={5} w={36}>
              <div text_size={11} text_color="secondary">
                {String(hour.hour).padStart(2, "0")}
              </div>
              <Icon name={glyph(hour.condition, hour.hour >= 6 && hour.hour < 20)} icon_size={15} />
              <div text_size={12} font_weight="medium">
                {degrees(hour.temperature)}
              </div>
            </div>
          ))}
        </div>
        <Footer>
          <div>{minutes === 0 ? "Updated just now" : `Updated ${minutes} min ago`}</div>
          <div>Open-Meteo</div>
        </Footer>
      </Card>
    );
  },
});
