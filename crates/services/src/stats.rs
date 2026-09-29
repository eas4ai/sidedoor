pub use domain::data::stats::*;
use std::collections::VecDeque;
use sysinfo::{Disks, System};

pub struct Sampler {
    system: System,
    disks: Disks,
    pub history: VecDeque<f32>,
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
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
