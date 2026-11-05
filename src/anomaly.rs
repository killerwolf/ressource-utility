//! Détection d'anomalies (pure, testable) et limiteur d'appels automatiques au LLM.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const GB: u64 = 1_000_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Cpu,
    Memory,
    Disk,
    Temperature,
    Process,
}

impl Kind {
    const ALL: [Kind; 5] = [
        Kind::Cpu,
        Kind::Memory,
        Kind::Disk,
        Kind::Temperature,
        Kind::Process,
    ];

    fn idx(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Cpu => "CPU élevé",
            Kind::Memory => "Mémoire saturée",
            Kind::Disk => "Disque presque plein",
            Kind::Temperature => "Température élevée",
            Kind::Process => "Processus gourmand",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Anomaly {
    pub kind: Kind,
    /// Processus principalement concerné (CPU / mémoire / processus).
    pub subject: Option<String>,
    pub detail: String,
}

impl Anomaly {
    /// Signature servant à dédupliquer les appels automatiques (type + processus).
    pub fn signature(&self) -> String {
        format!(
            "{:?}:{}",
            self.kind,
            self.subject.as_deref().unwrap_or("").to_lowercase()
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Thresholds {
    pub cpu_pct: f32,
    pub cpu_samples: u32,
    pub mem_pct: f32,
    pub disk_free_pct: f32,
    pub disk_free_gb: f32,
    pub temp_c: f32,
    pub proc_cpu_pct: f32,
    pub hysteresis: f32,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            cpu_pct: 85.0,
            cpu_samples: 5,
            mem_pct: 90.0,
            disk_free_pct: 10.0,
            disk_free_gb: 10.0,
            temp_c: 90.0,
            proc_cpu_pct: 80.0,
            hysteresis: 10.0,
        }
    }
}

/// Mesure instantanée utile au détecteur.
#[derive(Clone, Debug, Default)]
pub struct Sample {
    pub cpu: f32,
    pub mem_pct: f32,
    pub disk_free_pct: f32,
    pub disk_free_bytes: u64,
    pub temp: Option<f32>,
    pub top_cpu: Option<(String, f32)>,
    pub top_mem: Option<String>,
    /// La liste des processus a été rafraîchie pour cet échantillon.
    pub procs_fresh: bool,
}

#[derive(Debug, Default)]
pub struct Update {
    pub active: Vec<Anomaly>,
    pub started: Vec<Anomaly>,
}

#[derive(Default)]
struct Slot {
    on: bool,
    subject: Option<String>,
    detail: String,
}

pub struct Detector {
    th: Thresholds,
    slots: [Slot; 5],
    cpu_streak: u32,
    proc_track: Option<(String, u32)>,
}

impl Detector {
    pub fn new(th: Thresholds) -> Self {
        Self {
            th,
            slots: Default::default(),
            cpu_streak: 0,
            proc_track: None,
        }
    }

    fn slot_on(&self, k: Kind) -> bool {
        self.slots[k.idx()].on
    }

    fn start(&mut self, k: Kind, subject: Option<String>, detail: String, out: &mut Vec<Kind>) {
        self.slots[k.idx()] = Slot {
            on: true,
            subject,
            detail,
        };
        out.push(k);
    }

    fn stop(&mut self, k: Kind) {
        self.slots[k.idx()] = Slot::default();
    }

    pub fn update(&mut self, s: &Sample) -> Update {
        let th = self.th.clone();
        let mut started: Vec<Kind> = Vec::new();

        // CPU global : soutenu sur N échantillons, avec hystérésis à la baisse.
        if self.slot_on(Kind::Cpu) {
            if s.cpu < th.cpu_pct - th.hysteresis {
                self.stop(Kind::Cpu);
                self.cpu_streak = 0;
            }
        } else if s.cpu > th.cpu_pct {
            self.cpu_streak += 1;
            if self.cpu_streak >= th.cpu_samples {
                let subject = s.top_cpu.as_ref().map(|p| p.0.clone());
                let detail = format!("CPU à {:.0} % de façon soutenue", s.cpu);
                self.start(Kind::Cpu, subject, detail, &mut started);
            }
        } else {
            self.cpu_streak = 0;
        }

        // Mémoire.
        let mem_margin = th.hysteresis / 2.0;
        if self.slot_on(Kind::Memory) {
            if s.mem_pct < th.mem_pct - mem_margin {
                self.stop(Kind::Memory);
            }
        } else if s.mem_pct > th.mem_pct {
            let detail = format!("Mémoire utilisée à {:.0} %", s.mem_pct);
            self.start(Kind::Memory, s.top_mem.clone(), detail, &mut started);
        }

        // Disque.
        let low = s.disk_free_pct < th.disk_free_pct
            || s.disk_free_bytes < (th.disk_free_gb * GB as f32) as u64;
        if self.slot_on(Kind::Disk) {
            let recovered = s.disk_free_pct >= th.disk_free_pct + 2.0
                && s.disk_free_bytes >= ((th.disk_free_gb + 1.0) * GB as f32) as u64;
            if recovered {
                self.stop(Kind::Disk);
            }
        } else if low {
            let detail = format!(
                "Disque : {:.0} Go libres ({:.0} %)",
                s.disk_free_bytes as f32 / GB as f32,
                s.disk_free_pct
            );
            self.start(Kind::Disk, None, detail, &mut started);
        }

        // Température.
        if let Some(t) = s.temp {
            if self.slot_on(Kind::Temperature) {
                if t < th.temp_c - th.hysteresis / 2.0 {
                    self.stop(Kind::Temperature);
                }
            } else if t > th.temp_c {
                let detail = format!("Température à {t:.0} °C");
                self.start(Kind::Temperature, None, detail, &mut started);
            }
        }

        // Processus unique soutenu (uniquement quand la liste est fraîche).
        if s.procs_fresh {
            match &s.top_cpu {
                Some((name, pct)) => {
                    let same_active =
                        self.slots[Kind::Process.idx()].subject.as_deref() == Some(name.as_str());
                    if self.slot_on(Kind::Process) {
                        if !same_active || *pct < th.proc_cpu_pct - th.hysteresis {
                            self.stop(Kind::Process);
                            self.proc_track = None;
                        }
                    } else if *pct > th.proc_cpu_pct {
                        let n = match &self.proc_track {
                            Some((t, n)) if t == name => n + 1,
                            _ => 1,
                        };
                        self.proc_track = Some((name.clone(), n));
                        if n >= th.cpu_samples {
                            let detail = format!("{name} utilise {pct:.0} % de CPU");
                            self.start(Kind::Process, Some(name.clone()), detail, &mut started);
                        }
                    } else {
                        self.proc_track = None;
                    }
                }
                None => {
                    self.stop(Kind::Process);
                    self.proc_track = None;
                }
            }
        }

        let make = |k: Kind, slots: &[Slot; 5]| Anomaly {
            kind: k,
            subject: slots[k.idx()].subject.clone(),
            detail: slots[k.idx()].detail.clone(),
        };
        Update {
            active: Kind::ALL
                .iter()
                .filter(|k| self.slot_on(**k))
                .map(|k| make(*k, &self.slots))
                .collect(),
            started: started.iter().map(|k| make(*k, &self.slots)).collect(),
        }
    }
}

/// Limiteur d'appels LLM automatiques : 1 appel / 30 min, et pas deux fois la
/// même signature en 2 h. Le temps est fourni par l'appelant (secondes UNIX).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Gate {
    pub global_secs: u64,
    pub signature_secs: u64,
    last_auto: Option<u64>,
    signatures: HashMap<String, u64>,
}

impl Default for Gate {
    fn default() -> Self {
        Self {
            global_secs: 30 * 60,
            signature_secs: 2 * 60 * 60,
            last_auto: None,
            signatures: HashMap::new(),
        }
    }
}

impl Gate {
    /// Retourne `true` (et enregistre l'appel) si un appel automatique est permis.
    pub fn try_auto(&mut self, now: u64, signatures: &[String]) -> bool {
        if let Some(last) = self.last_auto
            && now.saturating_sub(last) < self.global_secs
        {
            return false;
        }
        let fresh = signatures.iter().any(|s| {
            self.signatures
                .get(s)
                .is_none_or(|t| now.saturating_sub(*t) >= self.signature_secs)
        });
        if !fresh {
            return false;
        }
        self.last_auto = Some(now);
        for s in signatures {
            self.signatures.insert(s.clone(), now);
        }
        let keep = self.signature_secs;
        self.signatures.retain(|_, t| now.saturating_sub(*t) < keep);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calm() -> Sample {
        Sample {
            cpu: 10.0,
            mem_pct: 50.0,
            disk_free_pct: 40.0,
            disk_free_bytes: 200 * GB,
            ..Default::default()
        }
    }

    #[test]
    fn calm_system_has_no_anomaly() {
        let mut d = Detector::new(Thresholds::default());
        let u = d.update(&calm());
        assert!(u.active.is_empty() && u.started.is_empty());
    }

    #[test]
    fn cpu_needs_five_consecutive_samples() {
        let mut d = Detector::new(Thresholds::default());
        let hot = Sample {
            cpu: 95.0,
            ..calm()
        };
        for _ in 0..4 {
            assert!(d.update(&hot).started.is_empty());
        }
        let u = d.update(&hot);
        assert_eq!(u.started.len(), 1);
        assert_eq!(u.started[0].kind, Kind::Cpu);
        // Pas de nouveau "started" tant que l'anomalie reste active.
        let u = d.update(&hot);
        assert!(u.started.is_empty());
        assert_eq!(u.active.len(), 1);
    }

    #[test]
    fn cpu_streak_resets_on_dip() {
        let mut d = Detector::new(Thresholds::default());
        let hot = Sample {
            cpu: 95.0,
            ..calm()
        };
        for _ in 0..4 {
            d.update(&hot);
        }
        d.update(&calm());
        for _ in 0..4 {
            assert!(d.update(&hot).started.is_empty());
        }
    }

    #[test]
    fn cpu_hysteresis_prevents_flapping() {
        let mut d = Detector::new(Thresholds::default());
        let hot = Sample {
            cpu: 95.0,
            ..calm()
        };
        for _ in 0..5 {
            d.update(&hot);
        }
        // 80 % : sous le seuil de 85 mais au-dessus de 75 -> reste actif.
        let mid = Sample {
            cpu: 80.0,
            ..calm()
        };
        assert_eq!(d.update(&mid).active.len(), 1);
        // 70 % : désactivé.
        assert!(
            d.update(&Sample {
                cpu: 70.0,
                ..calm()
            })
            .active
            .is_empty()
        );
        // Il faut de nouveau 5 échantillons pour re-déclencher.
        for _ in 0..4 {
            assert!(d.update(&hot).started.is_empty());
        }
        assert_eq!(d.update(&hot).started.len(), 1);
    }

    #[test]
    fn memory_disk_temp_thresholds() {
        let mut d = Detector::new(Thresholds::default());
        let u = d.update(&Sample {
            mem_pct: 93.0,
            top_mem: Some("Chrome".into()),
            ..calm()
        });
        assert_eq!(u.started[0].kind, Kind::Memory);
        assert_eq!(u.started[0].signature(), "Memory:chrome");
        // Hystérésis mémoire : 88 % reste actif, 80 % se désactive.
        assert_eq!(
            d.update(&Sample {
                mem_pct: 88.0,
                ..calm()
            })
            .active
            .len(),
            1
        );
        assert!(
            d.update(&Sample {
                mem_pct: 80.0,
                ..calm()
            })
            .active
            .is_empty()
        );

        let u = d.update(&Sample {
            disk_free_pct: 20.0,
            disk_free_bytes: 5 * GB,
            ..calm()
        });
        assert_eq!(u.started[0].kind, Kind::Disk);

        let u = d.update(&Sample {
            temp: Some(95.0),
            ..calm()
        });
        assert!(u.started.iter().any(|a| a.kind == Kind::Temperature));
        assert!(
            !d.update(&Sample {
                temp: Some(70.0),
                ..calm()
            })
            .active
            .iter()
            .any(|a| a.kind == Kind::Temperature)
        );
    }

    #[test]
    fn single_process_sustained() {
        let mut d = Detector::new(Thresholds::default());
        let s = |pct: f32| Sample {
            procs_fresh: true,
            top_cpu: Some(("ffmpeg".into(), pct)),
            ..calm()
        };
        for _ in 0..4 {
            assert!(d.update(&s(150.0)).started.is_empty());
        }
        let u = d.update(&s(150.0));
        assert_eq!(u.started[0].kind, Kind::Process);
        assert_eq!(u.started[0].signature(), "Process:ffmpeg");
        // Les échantillons sans rafraîchissement de processus ne changent rien.
        assert_eq!(d.update(&calm()).active.len(), 1);
        // Chute sous 70 % -> terminé.
        assert!(d.update(&s(20.0)).active.is_empty());
    }

    #[test]
    fn gate_global_and_signature_cooldown() {
        let mut g = Gate::default();
        let a = vec!["Cpu:chrome".to_string()];
        let b = vec!["Memory:safari".to_string()];
        assert!(g.try_auto(1_000, &a));
        // Moins de 30 min : refusé même pour une autre signature.
        assert!(!g.try_auto(1_000 + 29 * 60, &b));
        // Après 30 min : autre signature ok.
        assert!(g.try_auto(1_000 + 31 * 60, &b));
        // Même signature que la première, 1 h 36 après : refusée (< 2 h).
        assert!(!g.try_auto(1_000 + 96 * 60, &a));
        // Après 2 h : acceptée.
        assert!(g.try_auto(1_000 + 2 * 3600 + 3600, &a));
    }

    #[test]
    fn gate_accepts_if_any_signature_is_new() {
        let mut g = Gate::default();
        assert!(g.try_auto(0, &["Cpu:x".into()]));
        let both = vec!["Cpu:x".to_string(), "Process:x".to_string()];
        assert!(g.try_auto(40 * 60, &both));
        assert!(!g.try_auto(80 * 60, &both));
    }
}
