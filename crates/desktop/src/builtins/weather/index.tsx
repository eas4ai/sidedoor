/** @jsxImportSource ../../../../../sdk/src */
import {
  Card,
  Footer,
  Icon,
  Title,
  definePlugin,
  useData,
  type WeatherCondition,
} from "../../../../../sdk/src";

function glyph(condition: WeatherCondition, day: boolean): string {
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
// Rust rounds halves away from zero, including negative temperatures.
const degrees = (value: number) =>
  `${Math.sign(value) * Math.round(Math.abs(value))}°`;

export default definePlugin({
  name: "Weather",
  icon: "cloud",
  width: 300,
  height: 190,
  data: ["weather"],
  tile: () => {
    const data = useData("weather");
    const ready = data?.status === "ready" ? data.weather : null;
    return (
      <div flex flex_col items_center gap={3} magnify>
        <Icon
          name={
            ready
              ? glyph(ready.condition, ready.is_day)
              : data?.status === "failed"
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
  card: () => {
    const data = useData("weather");
    const location = data?.location.name ?? "Weather";
    if (data?.status !== "ready")
      return (
        <Card gap={4}>
          <Title>{location}</Title>
          <div text_size={12} text_color="secondary">
            {data?.status === "failed" ? data.message : "Loading weather…"}
          </div>
        </Card>
      );
    const weather = data.weather;
    const updated =
      data.updatedMinutes === 0
        ? "Updated just now"
        : `Updated ${data.updatedMinutes} min ago`;
    return (
      <Card gap={0}>
        <div flex items_start justify_between>
          <div flex flex_col>
            <Title>{location}</Title>
            <div text_size={12} text_color="secondary">
              {data.conditionLabel}
            </div>
          </div>
          <div flex items_center gap={8}>
            <Icon
              name={glyph(weather.condition, weather.is_day)}
              icon_size={24}
            />
            <div text_size={28} font_weight={300}>
              {degrees(weather.temperature)}
            </div>
          </div>
        </div>
        <div mt={2} text_size={12} text_color="secondary">
          High {degrees(weather.high)}, low {degrees(weather.low)}
        </div>
        <div mt={10} flex justify_between>
          {weather.hours.map((hour) => (
            <div key={hour.hour} flex flex_col items_center gap={5} w={36}>
              <div text_size={11} text_color="secondary">
                {String(hour.hour).padStart(2, "0")}
              </div>
              <Icon
                name={glyph(hour.condition, hour.hour >= 6 && hour.hour < 20)}
                icon_size={15}
              />
              <div text_size={12} font_weight="medium">
                {degrees(hour.temperature)}
              </div>
            </div>
          ))}
        </div>
        <Footer>
          <div>{updated}</div>
          <div>Open-Meteo</div>
        </Footer>
      </Card>
    );
  },
});
