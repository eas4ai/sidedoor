//! CPU, memory and startup-disk usage.

use std::collections::VecDeque;
use sysinfo::{Disks, System};

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

pub struct Sampler {
    system: System,
    disks: Disks,
    pub history: VecDeque<f32>,
}

impl Sampler {
    pub fn new() -> Self {
        let mut system = System::new();
        // CPU usage is a delta between refreshes; prime the first one.
        system.refresh_cpu_usage();
        Self {
            system,
            disks: Disks::new_with_refreshed_list(),
            history: VecDeque::with_capacity(HISTORY_LEN),
        }
    }

    pub fn sample(&mut self) -> Snapshot {
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        self.disks.refresh();

        let cpu = self.system.global_cpu_usage();
        if self.history.len() == HISTORY_LEN {
            self.history.pop_front();
        }
        self.history.push_back(cpu);

        #[cfg(target_os = "windows")]
        let root = std::path::PathBuf::from(format!(
            "{}\\",
            std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into())
        ));
        #[cfg(not(target_os = "windows"))]
        let root = std::path::PathBuf::from("/");
        let startup = self.disks.iter().find(|disk| disk.mount_point() == root);
        let (disk_total, disk_free) =
            startup.map_or((0, 0), |disk| (disk.total_space(), disk.available_space()));

        Snapshot {
            cpu,
            memory_used: self.system.used_memory(),
            memory_total: self.system.total_memory(),
            disk_used: disk_total.saturating_sub(disk_free),
            disk_total,
        }
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
