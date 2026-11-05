//! Construction des prompts : courts, denses, pour économiser les tokens.

use crate::anomaly::Anomaly;
use crate::metrics::{ProcInfo, ProcTop, Snapshot};

pub const SYSTEM: &str = "Tu es un expert macOS. Réponds en français, 3 puces max, ≤ 60 mots au total, concret et actionnable (ex. « Ferme X », « Y est normal, c'est … »). Aucune introduction.";
pub const SYSTEM_PROCESS: &str = "Tu es un expert macOS. Réponds en français en 2 phrases max (≤ 40 mots) : rôle du processus, est-il normal, faut-il le fermer ?";

pub const MAX_TOKENS_ANALYSIS: u32 = 200;
pub const MAX_TOKENS_PROCESS: u32 = 120;

fn short(name: &str) -> String {
    name.chars().take(28).collect()
}

fn proc_line(label: &str, list: &[ProcInfo]) -> String {
    let items: Vec<String> = list
        .iter()
        .map(|p| format!("{} {:.0}% {}MB", short(&p.name), p.cpu, p.mem / 1_000_000))
        .collect();
    format!("{label}: {}", items.join("; "))
}

/// Métriques sur une ligne + top 5 processus par CPU et par mémoire.
pub fn analysis(snap: &Snapshot, top: &ProcTop, anomalies: &[Anomaly]) -> String {
    let gb = |b: u64| b as f64 / 1e9;
    let temp = snap.temp.map_or("n/d".to_string(), |t| format!("{t:.0}C"));
    let mut out = format!(
        "CPU {:.0}% | RAM {:.1}/{:.0}Go ({:.0}%) | Disque libre {:.0}Go ({:.0}%) | Temp {temp}",
        snap.cpu,
        gb(snap.mem_used),
        gb(snap.mem_total),
        snap.mem_pct(),
        gb(snap.disk_avail),
        snap.disk_free_pct(),
    );
    if !anomalies.is_empty() {
        let a: Vec<&str> = anomalies.iter().map(|a| a.kind.label()).collect();
        out.push_str(&format!("\nAlertes: {}", a.join(", ")));
    }
    out.push('\n');
    out.push_str(&proc_line("CPU", &top.by_cpu));
    out.push('\n');
    out.push_str(&proc_line("RAM", &top.by_mem));
    out
}

pub fn process(name: &str, info: Option<&ProcInfo>) -> String {
    match info {
        Some(p) => format!(
            "Processus macOS « {} » ({:.0}% CPU, {} Mo). C'est quoi, est-ce normal ?",
            short(name),
            p.cpu,
            p.mem / 1_000_000
        ),
        None => format!(
            "Processus macOS « {} ». C'est quoi, est-ce normal ?",
            short(name)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anomaly::Kind;

    fn p(name: &str, cpu: f32, mem: u64) -> ProcInfo {
        ProcInfo {
            name: name.into(),
            cpu,
            mem,
            count: 1,
        }
    }

    #[test]
    fn analysis_prompt_is_compact() {
        let snap = Snapshot {
            cpu: 92.0,
            mem_used: 14_200_000_000,
            mem_total: 16_000_000_000,
            disk_avail: 8_000_000_000,
            disk_total: 500_000_000_000,
            temp: Some(78.0),
            procs: None,
        };
        let list: Vec<ProcInfo> = (0..5)
            .map(|i| p(&format!("Process{i}"), 50.0, 800_000_000))
            .collect();
        let top = ProcTop {
            by_cpu: list.clone(),
            by_mem: list,
        };
        let a = Anomaly {
            kind: Kind::Cpu,
            subject: Some("Process0".into()),
            detail: String::new(),
        };
        let s = analysis(&snap, &top, &[a]);
        assert!(s.starts_with("CPU 92% | RAM 14.2/16Go (89%)"));
        assert!(s.contains("Process0 50% 800MB"));
        assert!(s.contains("Alertes: CPU élevé"));
        assert!(s.chars().count() < 600, "prompt trop long: {}", s.len());
        assert!(SYSTEM.chars().count() < 250);
    }

    #[test]
    fn process_prompt_is_short() {
        let s = process("mds_stores", Some(&p("mds_stores", 12.0, 300_000_000)));
        assert!(s.contains("mds_stores") && s.chars().count() < 120);
        assert!(process("x", None).chars().count() < 80);
    }
}
