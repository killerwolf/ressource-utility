//! Collecte des métriques système (sysinfo), volontairement minimale.

use std::collections::HashMap;
use sysinfo::{
    Components, DiskRefreshKind, Disks, MemoryRefreshKind, ProcessRefreshKind, ProcessesToUpdate,
    System,
};

#[derive(Clone, Debug)]
pub struct ProcInfo {
    pub name: String,
    /// % CPU façon Moniteur d'activité (100 % = un cœur).
    pub cpu: f32,
    pub mem: u64,
    /// Nombre de processus regroupés sous ce nom.
    pub count: u32,
}

#[derive(Clone, Debug, Default)]
pub struct ProcTop {
    pub by_cpu: Vec<ProcInfo>,
    pub by_mem: Vec<ProcInfo>,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub cpu: f32,
    pub mem_used: u64,
    pub mem_total: u64,
    pub disk_avail: u64,
    pub disk_total: u64,
    pub temp: Option<f32>,
    /// Présent uniquement quand la liste des processus a été rafraîchie.
    pub procs: Option<ProcTop>,
}

impl Snapshot {
    pub fn mem_pct(&self) -> f32 {
        pct(self.mem_used, self.mem_total)
    }
    pub fn disk_free_pct(&self) -> f32 {
        pct(self.disk_avail, self.disk_total)
    }
}

fn pct(a: u64, b: u64) -> f32 {
    if b == 0 {
        0.0
    } else {
        a as f32 * 100.0 / b as f32
    }
}

/// Format français : « 10,2 Go », « 16 Go », « 120 Go ».
pub fn fmt_go(bytes: u64) -> String {
    let gb = bytes as f64 / 1_000_000_000.0;
    if gb >= 100.0 {
        return format!("{gb:.0} Go");
    }
    let s = format!("{gb:.1}").replace('.', ",");
    format!("{} Go", s.strip_suffix(",0").unwrap_or(&s))
}

/// Mémoire d'un processus : « 1,2 Go » ou « 340 Mo ».
pub fn fmt_mem(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        fmt_go(bytes)
    } else {
        format!("{} Mo", bytes / 1_000_000)
    }
}

fn proc_kind() -> ProcessRefreshKind {
    ProcessRefreshKind::nothing().with_cpu().with_memory()
}

const TEMP_KEYWORDS: [&str; 4] = ["cpu", "tdie", "soc", "core"];

/// Éléments coûteux à relever en plus du CPU global et de la mémoire.
#[derive(Clone, Copy, Default)]
pub struct Refresh {
    pub procs: bool,
    pub temp: bool,
    pub disk: bool,
}

impl Refresh {
    pub const ALL: Refresh = Refresh {
        procs: true,
        temp: true,
        disk: true,
    };
    pub const NONE: Refresh = Refresh {
        procs: false,
        temp: false,
        disk: false,
    };
}

pub struct Sampler {
    sys: System,
    disks: Option<Disks>,
    components: Option<Components>,
    temp: Option<f32>,
    disk: (u64, u64),
}

impl Sampler {
    pub fn new() -> Result<Self, sysinfo::Error> {
        let mut sys = System::new()?;
        // Amorçage : le CPU (global et par processus) se calcule entre deux relevés.
        sys.refresh_cpu_usage();
        sys.refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram());
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, proc_kind());
        Ok(Self {
            sys,
            disks: Disks::new_with_refreshed_list_specifics(
                DiskRefreshKind::nothing().with_storage(),
            )
            .ok(),
            components: Components::new_with_refreshed_list().ok(),
            temp: None,
            disk: (0, 0),
        })
    }

    /// CPU global et mémoire à chaque appel ; le reste selon `r` (les valeurs
    /// disque et température sont conservées entre deux relevés).
    pub fn sample(&mut self, r: Refresh) -> Snapshot {
        self.sys.refresh_cpu_usage();
        self.sys
            .refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram());
        if r.disk || self.disk == (0, 0) {
            if let Some(d) = &mut self.disks {
                d.refresh_specifics(true, DiskRefreshKind::nothing().with_storage());
            }
            self.disk = self.root_disk();
        }
        let (disk_avail, disk_total) = self.disk;
        if r.temp {
            self.temp = self.read_temp();
        }
        let procs = r.procs.then(|| self.top_processes());
        let total = self.sys.total_memory();
        Snapshot {
            cpu: self.sys.global_cpu_usage(),
            mem_used: total.saturating_sub(self.sys.available_memory()),
            mem_total: total,
            disk_avail,
            disk_total,
            temp: self.temp,
            procs,
        }
    }

    fn root_disk(&self) -> (u64, u64) {
        let Some(disks) = &self.disks else {
            return (0, 0);
        };
        let by_mount = |m: &str| {
            disks
                .iter()
                .find(|d| d.mount_point() == std::path::Path::new(m))
        };
        by_mount("/System/Volumes/Data")
            .or_else(|| by_mount("/"))
            .map(|d| (d.available_space(), d.total_space()))
            .unwrap_or((0, 0))
    }

    fn read_temp(&mut self) -> Option<f32> {
        let comps = self.components.as_mut()?;
        comps.refresh(true);
        comps
            .iter()
            .filter(|c| {
                let l = c.label().to_lowercase();
                TEMP_KEYWORDS.iter().any(|k| l.contains(k))
            })
            .filter_map(|c| c.temperature())
            .filter(|t| t.is_finite() && *t > 0.0 && *t < 150.0)
            .fold(None, |acc: Option<f32>, t| {
                Some(acc.map_or(t, |a| a.max(t)))
            })
    }

    /// Liste brute des composants (pour `--dump`).
    pub fn raw_components(&self) -> Vec<(String, Option<f32>)> {
        self.components
            .iter()
            .flat_map(|c| c.iter())
            .map(|c| (c.label().to_string(), c.temperature()))
            .collect()
    }

    fn top_processes(&mut self) -> ProcTop {
        self.sys
            .refresh_processes_specifics(ProcessesToUpdate::All, true, proc_kind());
        let mut map: HashMap<String, ProcInfo> = HashMap::new();
        for p in self.sys.processes().values() {
            let name = p.name().to_string_lossy().into_owned();
            let e = map.entry(name.clone()).or_insert(ProcInfo {
                name,
                cpu: 0.0,
                mem: 0,
                count: 0,
            });
            e.cpu += p.cpu_usage();
            e.mem += p.memory();
            e.count += 1;
        }
        let all: Vec<ProcInfo> = map.into_values().collect();
        let mut by_cpu = all.clone();
        by_cpu.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
        by_cpu.truncate(5);
        let mut by_mem = all;
        by_mem.sort_by_key(|p| std::cmp::Reverse(p.mem));
        by_mem.truncate(5);
        ProcTop { by_cpu, by_mem }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_in_french() {
        assert_eq!(fmt_go(16_000_000_000), "16 Go");
        assert_eq!(fmt_go(10_200_000_000), "10,2 Go");
        assert_eq!(fmt_go(120_400_000_000), "120 Go");
        assert_eq!(fmt_mem(340_000_000), "340 Mo");
        assert_eq!(fmt_mem(1_200_000_000), "1,2 Go");
    }
}
