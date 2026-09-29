//! Native services exposed to TSX plugins. Presentation stays in the plugins.

use crate::{
    dock::{Dock, WeatherState},
    plugin::DataSource,
    stats,
};
use serde_json::{Value, json};

pub fn snapshot(dock: &Dock, source: DataSource) -> Value {
    match source {
        DataSource::Weather => {
            let location = &dock.location;
            match &dock.weather {
                WeatherState::Loading => json!({"status":"loading", "location":location}),
                WeatherState::Failed(message) => json!({"status":"failed", "location":location, "message":message.as_ref()}),
                WeatherState::Ready { weather, updated } => json!({
                    "status":"ready", "location":location, "weather":weather,
                    "conditionLabel":weather.condition.label(), "updatedMinutes":updated.elapsed().as_secs() / 60,
                }),
            }
        }
        DataSource::Stats => dock.stats.as_ref().map_or(Value::Null, |stats| json!({
            "cpu":stats.cpu, "memoryPercent":stats.memory_percent(), "diskPercent":stats.disk_percent(),
            "memoryUsed":stats::format_memory(stats.memory_used), "memoryTotal":stats::format_memory(stats.memory_total),
            "diskFree":stats::format_bytes(stats.disk_total.saturating_sub(stats.disk_used)),
            "history":dock.cpu_history, "interval":crate::dock::STATS_INTERVAL.as_secs(),
        })),
        DataSource::Clipboard => {
            let now = crate::clipboard::now_secs();
            let entries: Vec<_> = dock.history.entries.iter().take(5).map(|entry| json!({
                "id":entry.id, "kind":entry.kind, "title":entry.kind.title(),
                "age":entry.age_label(now), "source":entry.source,
            })).collect();
            json!({"count":dock.history.len(), "entries":entries, "clearArmed":dock.is_clear_armed()})
        }
    }
}
