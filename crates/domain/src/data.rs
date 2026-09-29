//! Data exchanged between native services and presentation.
pub mod stats {
    /// Samples kept for the CPU sparkline.
    pub const HISTORY_LEN: usize = 30;

    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct Snapshot {
        /// 0–100.
        pub cpu: f32,
        pub memory_used: u64,
        pub memory_total: u64,
        pub disk_used: u64,
        pub disk_total: u64,
    }

    impl Snapshot {
        pub fn memory_percent(&self) -> f32 {
            percent(self.memory_used, self.memory_total)
        }

        pub fn disk_percent(&self) -> f32 {
            percent(self.disk_used, self.disk_total)
        }
    }

    fn percent(used: u64, total: u64) -> f32 {
        if total == 0 {
            0.0
        } else {
            (used as f64 / total as f64 * 100.0) as f32
        }
    }

    /// Formats storage the way Finder does (decimal units).
    pub fn format_bytes(bytes: u64) -> String {
        format_gb(bytes as f64 / 1e9)
    }

    /// Formats memory the way About This Mac does (binary units, shown as GB).
    pub fn format_memory(bytes: u64) -> String {
        format_gb(bytes as f64 / (1u64 << 30) as f64)
    }

    fn format_gb(gb: f64) -> String {
        if gb >= 100.0 {
            format!("{gb:.0} GB")
        } else {
            format!("{gb:.1} GB")
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn percentages_handle_empty_totals() {
            let snapshot = Snapshot {
                memory_used: 8,
                memory_total: 32,
                ..Default::default()
            };
            assert_eq!(snapshot.memory_percent(), 25.0);
            assert_eq!(snapshot.disk_percent(), 0.0);
        }

        #[test]
        fn formats_like_finder() {
            assert_eq!(format_bytes(19_500_000_000), "19.5 GB");
            assert_eq!(format_bytes(482_000_000_000), "482 GB");
            assert_eq!(format_memory(24 << 30), "24.0 GB");
        }
    }
}
pub mod weather {
    use crate::config::WeatherLocation;
    use serde::{Deserialize, Serialize};
    #[derive(Clone, Debug, PartialEq, Serialize)]
    pub struct Weather {
        pub temperature: f64,
        pub high: f64,
        pub low: f64,
        pub condition: Condition,
        pub is_day: bool,
        pub hours: Vec<Hour>,
    }

    #[derive(Clone, Debug, PartialEq, Serialize)]
    pub struct Hour {
        /// Local hour of day, 0–23.
        pub hour: u32,
        pub temperature: f64,
        pub condition: Condition,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
    #[serde(rename_all = "snake_case")]
    pub enum Condition {
        Clear,
        PartlyCloudy,
        Cloudy,
        Fog,
        Drizzle,
        Rain,
        Snow,
        Thunderstorm,
    }

    impl Condition {
        /// Maps a WMO weather interpretation code.
        pub fn from_wmo(code: u32) -> Self {
            match code {
                0 => Self::Clear,
                1 | 2 => Self::PartlyCloudy,
                45 | 48 => Self::Fog,
                51..=57 => Self::Drizzle,
                61..=67 | 80..=82 => Self::Rain,
                71..=77 | 85 | 86 => Self::Snow,
                95..=99 => Self::Thunderstorm,
                _ => Self::Cloudy,
            }
        }

        pub fn label(self) -> &'static str {
            match self {
                Self::Clear => "Clear",
                Self::PartlyCloudy => "Partly Cloudy",
                Self::Cloudy => "Cloudy",
                Self::Fog => "Fog",
                Self::Drizzle => "Drizzle",
                Self::Rain => "Rain",
                Self::Snow => "Snow",
                Self::Thunderstorm => "Thunderstorms",
            }
        }
    }

    /// A place the weather can be shown for, from Open-Meteo's geocoder.
    #[derive(Clone, Debug, PartialEq, Deserialize)]
    pub struct Place {
        pub name: String,
        pub latitude: f64,
        pub longitude: f64,
        /// State or region, when the geocoder knows it.
        #[serde(default, rename = "admin1")]
        pub region: Option<String>,
        #[serde(default)]
        pub country: Option<String>,
    }

    impl Place {
        /// "North Denmark, Denmark": what tells same-named places apart.
        pub fn detail(&self) -> String {
            [&self.region, &self.country]
                .into_iter()
                .flatten()
                .filter(|part| **part != self.name)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        }

        pub fn location(&self) -> WeatherLocation {
            WeatherLocation {
                name: self.name.clone(),
                latitude: self.latitude,
                longitude: self.longitude,
            }
        }
    }
}
