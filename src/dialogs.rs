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

/// Dialogue à boutons ; le dernier est le bouton par défaut. Retourne le
/// libellé du bouton choisi (bloquant : hors du thread principal).
pub fn choose(text: &str, buttons: &[&str]) -> Option<String> {
    let list: Vec<String> = buttons
        .iter()
        .map(|b| format!("\"{}\"", escape(b)))
        .collect();
    let default = buttons.last()?;
    let script = format!(
        "display dialog \"{}\" buttons {{{}}} default button \"{}\" with title \"{TITLE}\"",
        escape(text),
        list.join(", "),
        escape(default),
    );
    match run(&script) {
        Ok((true, out)) => parse_button(&out),
        _ => None,
    }
}

/// Extrait le bouton de `button returned:X` (avec un éventuel `, gave up:false`).
pub fn parse_button(stdout: &str) -> Option<String> {
    const MARKER: &str = "button returned:";
    let rest = &stdout[stdout.find(MARKER)? + MARKER.len()..];
    let rest = rest.lines().next().unwrap_or(rest);
    let rest = rest.split(", gave up:").next().unwrap_or(rest);
    Some(rest.trim().to_string())
}

/// Affiche un texte dans une boîte de dialogue (bloquant).
pub fn show_text(text: &str) {
    let script = format!(
        "display dialog \"{}\" buttons {{\"OK\"}} default button \"OK\" with title \"{TITLE} — Bilan\"",
        escape(text)
    );
    let _ = run(&script);
}

/// Confirmation à deux boutons ; « Annuler » est le bouton par défaut.
pub fn confirm(text: &str, ok_label: &str) -> bool {
    let script = format!(
        "display dialog \"{}\" buttons {{\"Annuler\", \"{}\"}} default button \"Annuler\" cancel button \"Annuler\" with icon caution with title \"{TITLE}\"",
        escape(text),
        escape(ok_label)
    );
    matches!(run(&script), Ok((true, out)) if out.contains(&format!("button returned:{ok_label}")))
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
    fn parses_button_returned() {
        assert_eq!(
            parse_button("button returned:Forcer à quitter…\n"),
            Some("Forcer à quitter…".into())
        );
        assert_eq!(
            parse_button("button returned:OK, gave up:false\n"),
            Some("OK".into())
        );
        assert_eq!(parse_button("rien"), None);
    }
}
