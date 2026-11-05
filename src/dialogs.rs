//! Boîtes de dialogue et notifications via `osascript` (aucune dépendance GUI).

use std::process::{Command, Stdio};

const TITLE: &str = "Ressource Utility";

/// Échappe une chaîne pour un littéral AppleScript entre guillemets.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

/// Extrait la valeur après `text returned:` de la sortie de `display dialog`.
/// Ex. : `button returned:OK, text returned:abc` -> `abc`.
pub fn parse_text_returned(stdout: &str) -> Option<String> {
    const MARKER: &str = "text returned:";
    let rest = &stdout[stdout.find(MARKER)? + MARKER.len()..];
    let rest = rest.strip_suffix('\n').unwrap_or(rest);
    let rest = rest.strip_suffix(", gave up:false").unwrap_or(rest);
    Some(rest.to_string())
}

#[derive(Debug, PartialEq)]
pub enum DialogResult {
    Text(String),
    Cancelled,
    Failed(String),
}

fn run(script: &str) -> Result<(bool, String), String> {
    let out = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("osascript indisponible : {e}"))?;
    Ok((
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    ))
}

/// Dialogue de saisie (bloquant : à appeler hors du thread principal).
pub fn ask_text(prompt: &str, default: &str, hidden: bool) -> DialogResult {
    let script = format!(
        "display dialog \"{}\" default answer \"{}\"{} with title \"{TITLE}\"",
        escape(prompt),
        escape(default),
        if hidden { " with hidden answer" } else { "" },
    );
    match run(&script) {
        Ok((true, out)) => parse_text_returned(&out)
            .map(DialogResult::Text)
            .unwrap_or(DialogResult::Failed("Réponse illisible".into())),
        // Code de sortie 1 : bouton Annuler.
        Ok((false, _)) => DialogResult::Cancelled,
        Err(e) => DialogResult::Failed(e),
    }
}

/// Affiche un texte dans une boîte de dialogue (bloquant).
pub fn show_text(text: &str) {
    let script = format!(
        "display dialog \"{}\" buttons {{\"OK\"}} default button \"OK\" with title \"{TITLE} — Bilan\"",
        escape(text)
    );
    let _ = run(&script);
}

/// Notification système, sans bloquer l'appelant.
pub fn notify(message: &str) {
    let script = format!(
        "display notification \"{}\" with title \"{TITLE}\"",
        escape(message)
    );
    std::thread::spawn(move || {
        let _ = run(&script);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_quotes_and_backslashes() {
        assert_eq!(escape(r#"a"b\c"#), r#"a\"b\\c"#);
        assert_eq!(escape("plain é"), "plain é");
        assert_eq!(escape("a\rb"), "a\\rb");
        // Un backslash suivi d'un guillemet ne doit pas "s'échapper" du littéral.
        assert_eq!(escape("\\\""), "\\\\\\\"");
    }

    #[test]
    fn parses_text_returned() {
        assert_eq!(
            parse_text_returned("button returned:OK, text returned:sk-abc123\n"),
            Some("sk-abc123".into())
        );
        assert_eq!(
            parse_text_returned("button returned:OK, text returned:\n"),
            Some(String::new())
        );
        assert_eq!(
            parse_text_returned("button returned:OK, text returned:a, b, gave up:false\n"),
            Some("a, b".into())
        );
        assert_eq!(parse_text_returned("button returned:OK\n"), None);
    }
}
