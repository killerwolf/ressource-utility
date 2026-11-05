//! Logique d'interface pure (testable) : phrase d'état, liste des processus,
//! boutons des dialogues, fraîcheur du bilan.

use crate::anomaly::{Anomaly, Kind};
use crate::known::Verdict;
use crate::metrics::{ProcInfo, ProcTop, Snapshot, fmt_go, fmt_mem};
use crate::security::{self, Finding};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Ok,
    Watch,
    Warn,
    Hot,
    Suspect,
}

impl Severity {
    pub fn emoji(self) -> &'static str {
        match self {
            Severity::Ok => "✅",
            Severity::Watch => "👀",
            Severity::Warn => "⚠️",
            Severity::Hot => "🔥",
            Severity::Suspect => "🛑",
        }
    }

    /// Préfixe du titre dans la barre des menus (🛑 > 🔥 > ⚠️), vide sinon.
    pub fn tray_prefix(self) -> &'static str {
        match self {
            Severity::Suspect => "🛑 ",
            Severity::Hot => "🔥 ",
            Severity::Warn => "⚠️ ",
            _ => "",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Headline {
    pub severity: Severity,
    pub text: String,
}

fn alert_severity(kind: Kind) -> Severity {
    match kind {
        Kind::Cpu | Kind::Temperature | Kind::Process => Severity::Hot,
        Kind::Memory | Kind::Disk => Severity::Warn,
    }
}

fn alert_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Cpu => "CPU",
        Kind::Memory => "mémoire",
        Kind::Disk => "disque",
        Kind::Temperature => "température",
        Kind::Process => "processus",
    }
}

/// Phrase d'une alerte seule (aussi utilisée pour les notifications).
pub fn alert_phrase(a: &Anomaly, snap: &Snapshot) -> String {
    let sev = alert_severity(a.kind).emoji();
    match a.kind {
        Kind::Cpu => format!("{sev} CPU très sollicité — {:.0} %", snap.cpu),
        Kind::Memory => format!("{sev} Mémoire presque pleine — {:.0} %", snap.mem_pct()),
        Kind::Disk => format!(
            "{sev} Disque presque plein — {} libres",
            fmt_go(snap.disk_avail)
        ),
        Kind::Temperature => match snap.temp {
            Some(t) => format!("{sev} Mac très chaud — {t:.0} °C"),
            None => format!("{sev} Mac très chaud"),
        },
        Kind::Process => match &a.subject {
            Some(n) => format!("{sev} {n} sollicite beaucoup le CPU"),
            None => format!("{sev} Un processus sollicite beaucoup le CPU"),
        },
    }
}

/// Phrase d'état unique, la plus grave d'abord.
pub fn headline(snap: &Snapshot, active: &[Anomaly], findings: &[Finding]) -> Headline {
    let suspects: Vec<&str> = findings
        .iter()
        .filter(|f| f.verdict == Verdict::Abnormal)
        .map(|f| f.name.as_str())
        .collect();
    if !suspects.is_empty() {
        let text = if suspects.len() == 1 {
            format!("🛑 Logiciel suspect : {}", suspects[0])
        } else {
            let shown: Vec<&str> = suspects.iter().take(3).copied().collect();
            format!(
                "🛑 {} logiciels suspects : {}",
                suspects.len(),
                shown.join(", ")
            )
        };
        return Headline {
            severity: Severity::Suspect,
            text,
        };
    }
    match active {
        [] => {}
        [one] => {
            return Headline {
                severity: alert_severity(one.kind),
                text: alert_phrase(one, snap),
            };
        }
        many => {
            let severity = many
                .iter()
                .map(|a| alert_severity(a.kind))
                .max()
                .unwrap_or(Severity::Warn);
            let names: Vec<&str> = many.iter().map(|a| alert_name(a.kind)).collect();
            return Headline {
                severity,
                text: format!(
                    "{} {} alertes — {}",
                    severity.emoji(),
                    many.len(),
                    names.join(", ")
                ),
            };
        }
    }
    let watch = findings
        .iter()
        .filter(|f| f.verdict == Verdict::Watch)
        .count();
    if watch > 0 {
        let text = if watch == 1 {
            "👀 1 élément à vérifier".to_string()
        } else {
            format!("👀 {watch} éléments à vérifier")
        };
        return Headline {
            severity: Severity::Watch,
            text,
        };
    }
    Headline {
        severity: Severity::Ok,
        text: "✅ Tout va bien".into(),
    }
}

// ---------------------------------------------------------------------------
// Liste « Processus »
// ---------------------------------------------------------------------------

pub const SLOTS: usize = 8;
const MAX_FINDINGS: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// Index dans la liste des constats de sécurité.
    Finding(usize),
    /// Index dans `ProcTop::by_cpu`.
    Proc(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub source: Source,
    pub label: String,
}

fn proc_label(p: &ProcInfo, v: Verdict) -> String {
    let name = if p.count > 1 {
        format!("{} ×{}", p.name, p.count)
    } else {
        p.name.clone()
    };
    format!(
        "{} {name} — {:.0} % CPU — {}",
        v.emoji(),
        p.cpu,
        fmt_mem(p.mem)
    )
}

/// Suspects d'abord (sécurité), puis les processus les plus gourmands, sans
/// doublon entre les deux.
pub fn entries(
    top: &ProcTop,
    findings: &[Finding],
    verdict_of: impl Fn(&ProcInfo) -> Verdict,
) -> Vec<Entry> {
    let mut out = Vec::new();
    for (i, f) in findings.iter().enumerate().take(MAX_FINDINGS) {
        let label = match &f.startup {
            Some(_) => format!("{} Démarrage : {}", f.verdict.emoji(), f.name),
            None => format!(
                "{} {} — {}",
                f.verdict.emoji(),
                f.name,
                if f.verdict == Verdict::Abnormal {
                    "Suspect"
                } else {
                    "À surveiller"
                }
            ),
        };
        out.push(Entry {
            source: Source::Finding(i),
            label,
        });
    }
    let taken: Vec<String> = findings
        .iter()
        .filter(|f| f.startup.is_none())
        .map(|f| crate::known::normalize(&f.name))
        .collect();
    for (i, p) in top.by_cpu.iter().enumerate() {
        if out.len() >= SLOTS {
            break;
        }
        if taken.contains(&crate::known::normalize(&p.name)) {
            continue;
        }
        out.push(Entry {
            source: Source::Proc(i),
            label: proc_label(p, verdict_of(p)),
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Boutons des dialogues
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Reveal,
    Kill,
    Refresh,
    Ok,
}

impl Button {
    pub fn label(self) -> &'static str {
        match self {
            Button::Reveal => "Afficher dans le Finder",
            Button::Kill => "Forcer à quitter…",
            Button::Refresh => "Actualiser",
            Button::Ok => "OK",
        }
    }

    pub fn from_label(s: &str) -> Option<Button> {
        [Button::Reveal, Button::Kill, Button::Refresh, Button::Ok]
            .into_iter()
            .find(|b| b.label() == s)
    }
}

/// Boutons d'un processus : OK seul pour un processus système (ou illisible et
/// non suspect) ; sinon Finder, et « Forcer à quitter » si on peut l'arrêter.
pub fn process_buttons(path: Option<&str>, suspect: bool, killable: bool) -> Vec<Button> {
    let system = match path {
        Some(p) => security::is_system_path(p),
        None => true,
    };
    if system && !suspect {
        return vec![Button::Ok];
    }
    let mut v = Vec::new();
    if path.is_some() {
        v.push(Button::Reveal);
    }
    if killable {
        v.push(Button::Kill);
    }
    v.push(Button::Ok);
    v
}

// ---------------------------------------------------------------------------
// Fraîcheur du bilan
// ---------------------------------------------------------------------------

pub const REPORT_MAX_AGE: Duration = Duration::from_secs(5 * 60);

/// Le bilan doit être refait s'il n'existe pas ou a plus de 5 minutes.
pub fn report_stale(last: Option<Instant>, now: Instant) -> bool {
    last.is_none_or(|t| now.duration_since(t) >= REPORT_MAX_AGE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap() -> Snapshot {
        Snapshot {
            cpu: 92.0,
            mem_used: 15_000_000_000,
            mem_total: 16_000_000_000,
            disk_avail: 14_000_000_000,
            disk_total: 250_000_000_000,
            temp: Some(93.0),
            procs: None,
        }
    }

    fn anomaly(kind: Kind) -> Anomaly {
        Anomaly {
            kind,
            subject: None,
            detail: String::new(),
        }
    }

    fn finding(name: &str, v: Verdict, startup: bool) -> Finding {
        Finding {
            name: name.into(),
            pid: Some(10),
            path: Some("/tmp/x".into()),
            verdict: v,
            reasons: vec![],
            miner: false,
            signature: None,
            ai: None,
            startup: startup.then(|| "/p.plist".to_string()),
        }
    }

    #[test]
    fn headline_ok_and_single_alerts() {
        assert_eq!(headline(&snap(), &[], &[]).text, "✅ Tout va bien");
        let h = headline(&snap(), &[anomaly(Kind::Disk)], &[]);
        assert_eq!(h.text, "⚠️ Disque presque plein — 14 Go libres");
        assert_eq!(h.severity, Severity::Warn);
        let h = headline(&snap(), &[anomaly(Kind::Cpu)], &[]);
        assert_eq!(h.text, "🔥 CPU très sollicité — 92 %");
        assert_eq!(h.severity.tray_prefix(), "🔥 ");
    }

    #[test]
    fn headline_priority_and_multi_alert_wording() {
        let two = headline(&snap(), &[anomaly(Kind::Disk), anomaly(Kind::Memory)], &[]);
        assert_eq!(two.text, "⚠️ 2 alertes — disque, mémoire");
        let mixed = headline(&snap(), &[anomaly(Kind::Disk), anomaly(Kind::Cpu)], &[]);
        assert_eq!(mixed.severity, Severity::Hot);
        assert_eq!(mixed.text, "🔥 2 alertes — disque, CPU");
        // Un suspect passe avant toute alerte.
        let s = headline(
            &snap(),
            &[anomaly(Kind::Cpu)],
            &[finding("xmrig", Verdict::Abnormal, false)],
        );
        assert_eq!(s.text, "🛑 Logiciel suspect : xmrig");
        assert_eq!(s.severity.tray_prefix(), "🛑 ");
        let many = headline(
            &snap(),
            &[],
            &[
                finding("a", Verdict::Abnormal, false),
                finding("b", Verdict::Abnormal, true),
            ],
        );
        assert_eq!(many.text, "🛑 2 logiciels suspects : a, b");
        // « À surveiller » seul, sans alerte.
        let w = headline(&snap(), &[], &[finding("c", Verdict::Watch, false)]);
        assert_eq!(w.text, "👀 1 élément à vérifier");
        assert_eq!(w.severity.tray_prefix(), "");
        // Une alerte l'emporte sur un simple « à surveiller ».
        let a = headline(
            &snap(),
            &[anomaly(Kind::Memory)],
            &[finding("c", Verdict::Watch, false)],
        );
        assert_eq!(a.severity, Severity::Warn);
    }

    fn p(name: &str, cpu: f32) -> ProcInfo {
        ProcInfo {
            name: name.into(),
            cpu,
            mem: 100_000_000,
            count: 1,
            pid: 1,
        }
    }

    #[test]
    fn entries_put_suspects_first_and_dedupe() {
        let top = ProcTop {
            by_cpu: vec![p("xmrig", 90.0), p("Safari", 30.0), p("Dock", 5.0)],
            by_mem: vec![],
            extra: vec![],
        };
        let findings = vec![
            finding("xmrig", Verdict::Abnormal, false),
            finding("com.foo.plist", Verdict::Watch, true),
        ];
        let e = entries(&top, &findings, |_| Verdict::Normal);
        assert_eq!(e[0].source, Source::Finding(0));
        assert_eq!(e[1].source, Source::Finding(1));
        assert!(e[0].label.starts_with("🛑 xmrig — Suspect"));
        assert!(e[1].label.starts_with("👀 Démarrage : com.foo.plist"));
        // xmrig n'apparaît pas une seconde fois dans les processus.
        let procs: Vec<Source> = e.iter().skip(2).map(|x| x.source).collect();
        assert_eq!(procs, vec![Source::Proc(1), Source::Proc(2)]);
        assert!(e[2].label.starts_with("✅ Safari — 30 % CPU"));
        assert!(e.len() <= SLOTS);
    }

    #[test]
    fn dialog_buttons_depend_on_path() {
        let sys = Some("/System/Library/CoreServices/Finder.app/Contents/MacOS/Finder");
        let app = Some("/Applications/Foo.app/Contents/MacOS/Foo");
        assert_eq!(process_buttons(sys, false, true), vec![Button::Ok]);
        assert_eq!(process_buttons(None, false, true), vec![Button::Ok]);
        assert_eq!(
            process_buttons(app, false, true),
            vec![Button::Reveal, Button::Kill, Button::Ok]
        );
        // Suspect, même sous un chemin système : toutes les actions possibles.
        assert_eq!(
            process_buttons(sys, true, true),
            vec![Button::Reveal, Button::Kill, Button::Ok]
        );
        // Élément de démarrage : pas d'arrêt possible.
        assert_eq!(
            process_buttons(app, true, false),
            vec![Button::Reveal, Button::Ok]
        );
        assert_eq!(Button::from_label("Forcer à quitter…"), Some(Button::Kill));
        assert_eq!(Button::from_label("?"), None);
    }

    #[test]
    fn report_staleness() {
        let t0 = Instant::now();
        assert!(report_stale(None, t0));
        assert!(!report_stale(Some(t0), t0 + Duration::from_secs(299)));
        assert!(report_stale(Some(t0), t0 + Duration::from_secs(300)));
    }
}
