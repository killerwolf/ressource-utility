//! Prompts compacts à format de sortie strict, et analyse tolérante des réponses.

use crate::known::{Verdict, normalize};
use crate::metrics::{ProcInfo, Snapshot};

pub const SYSTEM: &str = "Tu es un expert macOS. Une ligne par processus listé, au format exact : NOM|verdict|explication (verdict = normal, surveiller ou anormal ; explication de 15 mots max, français simple). Puis si utile une ligne : CONSEIL|action concrète de 20 mots max. Réponds UNIQUEMENT avec ces lignes, sans raisonnement, sans markdown, en français.";
pub const SYSTEM_PROCESS: &str = "Tu es un expert macOS. Réponds par UNE seule ligne au format exact : NOM|verdict|explication (verdict = normal, surveiller ou anormal ; explication de 25 mots max, français simple : c'est quoi, est-ce normal). Réponds UNIQUEMENT avec cette ligne, sans raisonnement, sans markdown, en français.";

pub const MAX_TOKENS_PROCESS: u32 = 120;

/// ~40 tokens par processus + 40, plafonné.
pub fn analysis_tokens(unknown: usize) -> u32 {
    (40 * unknown as u32 + 40).min(300)
}

fn short(name: &str) -> String {
    name.chars().take(28).collect()
}

/// Métriques sur une ligne + alertes + processus à juger (inconnus seulement).
pub fn analysis(snap: &Snapshot, alerts: &[&str], unknown: &[ProcInfo]) -> String {
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
    if !alerts.is_empty() {
        out.push_str(&format!("\nAlertes: {}", alerts.join(", ")));
    }
    out.push_str("\nProcessus à juger:");
    for p in unknown {
        out.push_str(&format!(
            "\n{} {:.0}% {}MB",
            short(&p.name),
            p.cpu,
            p.mem / 1_000_000
        ));
    }
    out
}

pub fn process(info: &ProcInfo) -> String {
    format!(
        "Processus à juger:\n{} {:.0}% {}MB",
        short(&info.name),
        info.cpu,
        info.mem / 1_000_000
    )
}

#[derive(Debug, Default, PartialEq)]
pub struct Reply {
    /// (nom tel qu'envoyé, verdict, explication)
    pub items: Vec<(String, Verdict, String)>,
    pub advice: Option<String>,
}

pub const UNREADABLE: &str =
    "Réponse illisible du modèle — essaie un autre modèle via « Changer le modèle… »";

fn clean(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || matches!(c, '*' | '-' | '`' | '•' | '#' | '_'))
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>().trim_end())
    }
}

/// Analyse tolérante : ignore tout ce qui n'a pas le bon format.
/// `sent` = noms envoyés au modèle ; les lignes sont rapprochées de ces noms.
pub fn parse_reply(text: &str, sent: &[String]) -> Result<Reply, &'static str> {
    let mut reply = Reply::default();
    for line in text.lines() {
        let line = clean(line);
        // Retire une numérotation « 1. » ou « 1) ».
        let digits = line.trim_start_matches(|c: char| c.is_ascii_digit());
        let line = match digits.strip_prefix(['.', ')']) {
            Some(rest) if digits.len() < line.len() => rest.trim(),
            _ => line,
        };
        let parts: Vec<&str> = line.split('|').map(clean).collect();
        if parts.len() < 2 || parts[0].is_empty() {
            continue;
        }
        if parts[0].eq_ignore_ascii_case("conseil") {
            let t = parts[1..].join(" ");
            if !t.is_empty() && reply.advice.is_none() {
                reply.advice = Some(truncate(&t, 200));
            }
            continue;
        }
        if parts.len() < 3 {
            continue;
        }
        let Some(verdict) = Verdict::parse(parts[1]) else {
            continue;
        };
        let key = normalize(parts[0]);
        let Some(name) = sent.iter().find(|n| normalize(n) == key) else {
            continue;
        };
        let expl = parts[2..].join(" ");
        if expl.is_empty() || reply.items.iter().any(|(n, _, _)| n == name) {
            continue;
        }
        reply
            .items
            .push((name.clone(), verdict, truncate(&expl, 200)));
    }
    if reply.items.is_empty() && reply.advice.is_none() {
        Err(UNREADABLE)
    } else {
        Ok(reply)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str, cpu: f32, mem: u64) -> ProcInfo {
        ProcInfo {
            name: name.into(),
            cpu,
            mem,
            count: 1,
        }
    }

    fn sent() -> Vec<String> {
        vec!["mystery_d".into(), "Foo Helper (GPU)".into()]
    }

    #[test]
    fn analysis_prompt_is_compact_and_lists_only_unknowns() {
        let snap = Snapshot {
            cpu: 92.0,
            mem_used: 14_200_000_000,
            mem_total: 16_000_000_000,
            disk_avail: 8_000_000_000,
            disk_total: 500_000_000_000,
            temp: Some(78.0),
            procs: None,
        };
        let unknown = vec![p("mystery_d", 50.0, 800_000_000)];
        let s = analysis(&snap, &["CPU élevé"], &unknown);
        assert!(s.starts_with("CPU 92% | RAM 14.2/16Go (89%)"));
        assert!(s.contains("Alertes: CPU élevé"));
        assert!(s.ends_with("mystery_d 50% 800MB"));
        assert!(s.chars().count() < 300);
        assert!(SYSTEM.contains("UNIQUEMENT"));
        assert_eq!(analysis_tokens(2), 120);
        assert_eq!(analysis_tokens(50), 300);
        assert!(process(&p("x", 1.0, 5_000_000)).contains("x 1% 5MB"));
    }

    #[test]
    fn parses_clean_reply() {
        let t = "mystery_d|surveiller|Service inconnu qui consomme du CPU.\nFoo Helper|normal|Partie de Foo.\nCONSEIL|Redémarre le Mac.";
        let r = parse_reply(t, &sent()).unwrap();
        assert_eq!(r.items.len(), 2);
        assert_eq!(
            r.items[0],
            (
                "mystery_d".into(),
                Verdict::Watch,
                "Service inconnu qui consomme du CPU.".into()
            )
        );
        // Renvoyé sous le nom envoyé, même sans la décoration.
        assert_eq!(r.items[1].0, "Foo Helper (GPU)");
        assert_eq!(r.advice.as_deref(), Some("Redémarre le Mac."));
    }

    #[test]
    fn parses_markdown_decorated_lines() {
        let t = "**MYSTERY_D** | **À surveiller** | Consomme beaucoup.\n- `Foo Helper`|Normal|Ok.\n1. conseil | Ferme Foo.";
        let r = parse_reply(t, &sent()).unwrap();
        assert_eq!(r.items[0].1, Verdict::Watch);
        assert_eq!(r.items[1].1, Verdict::Normal);
        assert_eq!(r.advice.as_deref(), Some("Ferme Foo."));
    }

    #[test]
    fn mixed_garbage_keeps_only_valid_lines() {
        let t = "Voici mon analyse :\n1. **Analyze the Request:** think\nunknown_proc|normal|pas envoyé\nmystery_d|peut-être|verdict invalide\nmystery_d|anormal|Très suspect.\nmystery_d|normal|doublon ignoré";
        let r = parse_reply(t, &sent()).unwrap();
        assert_eq!(r.items.len(), 1);
        assert_eq!(r.items[0].1, Verdict::Abnormal);
        assert!(r.advice.is_none());
    }

    #[test]
    fn pure_reasoning_is_an_error() {
        let t = "1. **Analyze the Request:** The user wants...\n2. Draft 1: mystery_d is normal\n   * Word count check: 14 words.\nFinal answer: ok";
        assert_eq!(parse_reply(t, &sent()), Err(UNREADABLE));
        assert_eq!(parse_reply("", &sent()), Err(UNREADABLE));
    }
}
