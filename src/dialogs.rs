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
}
