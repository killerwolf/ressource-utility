//! Bilan rendu localement en texte simple (aucun markdown, aucun texte brut du LLM).

use crate::anomaly::Thresholds;
use crate::config::Explanations;
use crate::known::{self, Known, Verdict};
use crate::metrics::{ProcInfo, ProcTop, Snapshot, fmt_go, fmt_mem};
use crate::security::{self, Finding};

const HEAVY_MEM: u64 = 2_500_000_000;

/// Verdict + explication d'un processus : base locale, puis cache LLM.
/// Un processus « normal » qui pèse lourd passe en « à surveiller ».
pub fn judge(p: &ProcInfo, cache: &Explanations, th: &Thresholds, findings: &[Finding]) -> Known {
    let key = known::normalize(&p.name);
    if let Some(f) = findings
        .iter()
        .find(|f| f.startup.is_none() && known::normalize(&f.name) == key)
    {
        return security::judged_known(f, &security::home());
    }
    let base = known::lookup(&p.name)
        .or_else(|| {
            cache.get(&p.name).map(|c| Known {
                verdict: c.verdict,
                text: c.text.clone(),
                security: false,
            })
        })
        .unwrap_or(Known {
            verdict: Verdict::Unknown,
            text: String::new(),
            security: false,
        });
    if base.verdict != Verdict::Normal {
        return base;
    }
    let extra = if p.mem >= HEAVY_MEM {
        Some(format!(
            "Il utilise {} de RAM : ferme ce qui ne sert pas.",
            fmt_mem(p.mem)
        ))
    } else if p.cpu >= th.proc_cpu_pct {
        Some(format!("Il utilise {:.0} % de CPU en ce moment.", p.cpu))
    } else {
        None
    };
    match extra {
        Some(e) => Known {
            verdict: Verdict::Watch,
            text: format!("{} {e}", base.text),
            security: false,
        },
        None => base,
    }
}

/// Processus affichés : top 5 CPU puis top mémoire non déjà listés (8 au plus).
pub fn rows(top: &ProcTop) -> Vec<&ProcInfo> {
    let mut out: Vec<&ProcInfo> = top.by_cpu.iter().collect();
    for p in &top.by_mem {
        if out.len() >= 8 {
            break;
        }
        if !out.iter().any(|q| q.name == p.name) {
            out.push(p);
        }
    }
    out
}

/// Processus à soumettre au LLM : ni connus localement, ni en cache (6 au plus).
pub fn unknown(top: &ProcTop, cache: &Explanations, findings: &[Finding]) -> Vec<ProcInfo> {
    rows(top)
        .into_iter()
        .filter(|p| {
            known::lookup(&p.name).is_none()
                && cache.get(&p.name).is_none()
                && !findings
                    .iter()
                    .any(|f| known::normalize(&f.name) == known::normalize(&p.name))
        })
        .take(6)
        .cloned()
        .collect()
}

#[derive(Default, Debug, PartialEq)]
pub struct Counts {
    pub normal: usize,
    pub watch: usize,
    pub abnormal: usize,
    pub unknown: usize,
    pub alerts: usize,
    pub suspects: usize,
}

pub struct Report {
    pub text: String,
    pub summary: String,
    pub notify: String,
    pub counts: Counts,
}

pub struct Inputs<'a> {
    pub snap: &'a Snapshot,
    pub top: &'a ProcTop,
    pub cache: &'a Explanations,
    pub th: &'a Thresholds,
    pub findings: &'a [Finding],
    pub has_key: bool,
    pub llm_advice: Option<&'a str>,
    pub error: Option<&'a str>,
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n > 1 { many } else { one })
}

pub fn build(i: &Inputs) -> Report {
    let (snap, th) = (i.snap, i.th);
    let top_cpu = i.top.by_cpu.first().map(|p| p.name.as_str());
    let top_mem = i.top.by_mem.first();
    let mut warns: Vec<String> = Vec::new();
    let mut oks: Vec<String> = Vec::new();
    let mut advice: Vec<String> = Vec::new();

    if snap.cpu > th.cpu_pct {
        let who = top_cpu
            .map(|n| format!(" (surtout {n})"))
            .unwrap_or_default();
        warns.push(format!("⚠️ CPU élevé : {:.0} %{who}", snap.cpu));
        if let Some(n) = top_cpu {
            advice.push(format!("Quitte {n} s'il ne te sert pas."));
        }
    } else {
        oks.push(format!("CPU {:.0} %", snap.cpu));
    }

    if snap.mem_pct() > th.mem_pct {
        let who = top_mem
            .map(|p| {
                format!(
                    " — plus gros consommateur : {} ({})",
                    known::strip_decor(&p.name),
                    fmt_mem(p.mem)
                )
            })
            .unwrap_or_default();
        warns.push(format!(
            "⚠️ Mémoire presque pleine : {:.0} %{who}",
            snap.mem_pct()
        ));
        if let Some(p) = top_mem {
            advice.push(format!(
                "Ferme {} ou ses onglets inutiles.",
                known::strip_decor(&p.name)
            ));
        }
    } else {
        oks.push(format!("Mémoire {:.0} %", snap.mem_pct()));
    }

    let low =
        snap.disk_free_pct() < th.disk_free_pct || (snap.disk_avail as f32) < th.disk_free_gb * 1e9;
    if low {
        warns.push(format!(
            "⚠️ Disque presque plein : {} libres ({:.0} %)",
            fmt_go(snap.disk_avail),
            snap.disk_free_pct()
        ));
        advice.push("Libère de l'espace disque (Réglages > Général > Stockage).".into());
    } else {
        oks.push(format!("Disque {} libres", fmt_go(snap.disk_avail)));
    }

    if let Some(t) = snap.temp {
        if t > th.temp_c {
            warns.push(format!("⚠️ Mac très chaud : {t:.0} °C"));
            advice.push("Laisse le Mac refroidir et ferme les applications lourdes.".into());
        } else {
            oks.push(format!("{t:.0} °C"));
        }
    }

    let mut text = String::from("État du Mac\n");
    for w in &warns {
        text.push_str(w);
        text.push('\n');
    }
    if !oks.is_empty() {
        text.push_str(&format!("✅ {} — normal\n", oks.join(" · ")));
    }

    let mut counts = Counts {
        alerts: warns.len(),
        suspects: i
            .findings
            .iter()
            .filter(|f| f.verdict == Verdict::Abnormal)
            .count(),
        ..Default::default()
    };
    let home = security::home();
    text.push_str("\nSécurité\n");
    if i.findings.is_empty() {
        text.push_str("✅ Aucun logiciel suspect détecté\n");
    }
    let mut sorted: Vec<&Finding> = i.findings.iter().collect();
    sorted.sort_by_key(|f| f.verdict != Verdict::Abnormal);
    for f in sorted {
        let emoji = f.verdict.emoji();
        match &f.startup {
            Some(_) => {
                let target = f
                    .path
                    .as_deref()
                    .map(|p| security::sanitize_path(p, &home))
                    .unwrap_or_default();
                text.push_str(&format!(
                    "{emoji} Démarrage automatique : {} → {target} ({})\n",
                    f.name,
                    f.reasons.join(", ")
                ));
            }
            None => {
                let loc = f
                    .path
                    .as_deref()
                    .map(|p| format!(" [{}]", security::sanitize_path(p, &home)))
                    .unwrap_or_default();
                text.push_str(&format!("{emoji} {} — {}{loc}\n", f.name, f.text()));
                if let Some(a) = &f.ai {
                    text.push_str(&format!("   Avis de l'IA : {a}\n"));
                }
            }
        }
    }
    text.push_str("\nCe qui tourne\n");
    for p in rows(i.top) {
        let k = judge(p, i.cache, th, i.findings);
        match k.verdict {
            Verdict::Normal => counts.normal += 1,
            Verdict::Watch => counts.watch += 1,
            Verdict::Abnormal => counts.abnormal += 1,
            Verdict::Unknown => counts.unknown += 1,
        }
        let count = if p.count > 1 {
            format!(" ×{}", p.count)
        } else {
            String::new()
        };
        let detail = if k.verdict == Verdict::Unknown {
            if i.has_key {
                "Pas encore identifié. Relance « Bilan de santé… »."
            } else {
                "Inconnu — configure une clé IA pour l'identifier."
            }
            .to_string()
        } else {
            k.text
        };
        text.push_str(&format!(
            "{} {}{count} — {}\n   {detail}\n",
            k.verdict.emoji(),
            known::strip_decor(&p.name),
            if k.security && k.verdict == Verdict::Abnormal {
                "Suspect"
            } else {
                k.verdict.label()
            }
        ));
    }

    let mut tips: Vec<String> = advice.into_iter().take(2).collect();
    if let Some(a) = i.llm_advice {
        tips.push(a.to_string());
    }
    if !tips.is_empty() {
        text.push('\n');
        for t in &tips {
            text.push_str(&format!("👉 Conseil : {t}\n"));
        }
    }
    if let Some(e) = i.error {
        text.push_str(&format!("\n⚠️ Analyse IA indisponible : {e}\n"));
    }

    let mut parts: Vec<String> = Vec::new();
    if counts.suspects > 0 {
        parts.push(plural(counts.suspects, "suspect", "suspects"));
    }
    if counts.alerts > 0 {
        parts.push(plural(counts.alerts, "alerte système", "alertes système"));
    }
    if counts.normal > 0 {
        parts.push(plural(counts.normal, "normal", "normaux"));
    }
    if counts.watch > 0 {
        parts.push(format!("{} à surveiller", counts.watch));
    }
    if counts.abnormal > 0 {
        parts.push(plural(counts.abnormal, "anormal", "anormaux"));
    }
    if counts.unknown > 0 {
        parts.push(plural(counts.unknown, "inconnu", "inconnus"));
    }
    let summary = if parts.is_empty() {
        "Bilan : en attente des mesures".to_string()
    } else {
        format!("Bilan : {}", parts.join(", "))
    };
    let notify = if counts.suspects > 0 {
        format!(
            "Bilan prêt : {}",
            plural(counts.suspects, "logiciel suspect", "logiciels suspects")
        )
    } else if counts.abnormal > 0 {
        format!(
            "Bilan prêt : {}",
            plural(counts.abnormal, "processus anormal", "processus anormaux")
        )
    } else if counts.watch > 0 {
        format!(
            "Bilan prêt : {} à surveiller",
            plural(counts.watch, "processus", "processus")
        )
    } else if counts.alerts > 0 {
        format!(
            "Bilan prêt : {}",
            plural(counts.alerts, "alerte système", "alertes système")
        )
    } else {
        "Bilan prêt : tout est normal".to_string()
    };

    Report {
        text: text.trim_end().to_string(),
        summary,
        notify,
        counts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CachedVerdict;

    fn p(name: &str, cpu: f32, mem: u64, count: u32) -> ProcInfo {
        ProcInfo {
            name: name.into(),
            cpu,
            mem,
            count,
            pid: 0,
        }
    }

    fn snap(cpu: f32, disk_avail: u64) -> Snapshot {
        Snapshot {
            cpu,
            mem_used: 8_300_000_000,
            mem_total: 16_000_000_000,
            disk_avail,
            disk_total: 250_000_000_000,
            temp: Some(45.0),
            procs: None,
        }
    }

    fn top() -> ProcTop {
        ProcTop {
            by_cpu: vec![
                p("avconferenced", 24.0, 40_000_000, 1),
                p("mystery_d", 10.0, 90_000_000, 1),
            ],
            by_mem: vec![p("Google Chrome Helper (Renderer)", 3.0, 3_700_000_000, 12)],
            extra: vec![],
        }
    }

    #[test]
    fn renders_report_snapshot() {
        let mut cache = Explanations::default();
        cache.0.insert(
            "mystery_d".into(),
            CachedVerdict {
                verdict: Verdict::Normal,
                text: "Petit service de fond.".into(),
            },
        );
        let th = Thresholds::default();
        let r = build(&Inputs {
            snap: &snap(22.0, 10_000_000_000),
            top: &top(),
            cache: &cache,
            th: &th,
            findings: &[],
            has_key: true,
            llm_advice: Some("Redémarre le Mac ce soir."),
            error: None,
        });
        let expected = "\
État du Mac
⚠️ Disque presque plein : 10 Go libres (4 %)
✅ CPU 22 % · Mémoire 52 % · 45 °C — normal

Sécurité
✅ Aucun logiciel suspect détecté

Ce qui tourne
✅ avconferenced — Normal
   Gère les appels audio et vidéo (FaceTime, Teams, Zoom…). Normal pendant un appel.
✅ mystery_d — Normal
   Petit service de fond.
👀 Google Chrome Helper ×12 — À surveiller
   Un onglet ou une extension de Google Chrome. Normal, mais beaucoup de RAM = trop d'onglets ouverts. Il utilise 3,7 Go de RAM : ferme ce qui ne sert pas.

👉 Conseil : Libère de l'espace disque (Réglages > Général > Stockage).
👉 Conseil : Redémarre le Mac ce soir.";
        assert_eq!(r.text, expected);
        assert_eq!(
            r.summary,
            "Bilan : 1 alerte système, 2 normaux, 1 à surveiller"
        );
        assert_eq!(r.notify, "Bilan prêt : 1 processus à surveiller");
        assert!(!r.text.contains('*') && !r.text.contains('#'));
    }

    #[test]
    fn unknown_processes_without_key() {
        let th = Thresholds::default();
        let cache = Explanations::default();
        let r = build(&Inputs {
            snap: &snap(95.0, 100_000_000_000),
            top: &top(),
            cache: &cache,
            th: &th,
            findings: &[],
            has_key: false,
            llm_advice: None,
            error: Some("Réseau indisponible"),
        });
        assert!(
            r.text
                .contains("⚠️ CPU élevé : 95 % (surtout avconferenced)")
        );
        assert!(r.text.contains(
            "❓ mystery_d — Inconnu\n   Inconnu — configure une clé IA pour l'identifier."
        ));
        assert!(
            r.text
                .contains("Analyse IA indisponible : Réseau indisponible")
        );
        assert_eq!(r.counts.unknown, 1);
    }

    #[test]
    fn all_known_means_nothing_to_ask() {
        let mut t = top();
        t.by_cpu.pop();
        assert!(unknown(&t, &Explanations::default(), &[]).is_empty());
        // Avec l'inconnu, il est proposé au LLM ; une fois en cache, plus.
        let mut cache = Explanations::default();
        assert_eq!(unknown(&top(), &cache, &[]).len(), 1);
        cache.0.insert(
            "mystery_d".into(),
            CachedVerdict {
                verdict: Verdict::Watch,
                text: "x".into(),
            },
        );
        assert!(unknown(&top(), &cache, &[]).is_empty());
    }

    #[test]
    fn cached_abnormal_is_not_downgraded() {
        let mut cache = Explanations::default();
        cache.0.insert(
            "mystery_d".into(),
            CachedVerdict {
                verdict: Verdict::Abnormal,
                text: "Suspect.".into(),
            },
        );
        let k = judge(
            &p("mystery_d", 1.0, 1, 1),
            &cache,
            &Thresholds::default(),
            &[],
        );
        assert_eq!(k.verdict, Verdict::Abnormal);
    }
}
