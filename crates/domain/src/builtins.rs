pub const WEATHER: &str = "builtin.weather";
pub const CLIPBOARD: &str = "builtin.clipboard";
pub const STATS: &str = "builtin.stats";

pub fn legacy_id(id: &str) -> Option<&'static str> {
    match id {
        "weather" => Some(WEATHER),
        "clipboard" => Some(CLIPBOARD),
        "stats" => Some(STATS),
        _ => None,
    }
}

pub fn contains(id: &str) -> bool {
    matches!(id, WEATHER | CLIPBOARD | STATS)
}
