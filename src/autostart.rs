//! Lancement au démarrage via un LaunchAgent utilisateur.

use std::path::{Path, PathBuf};

const LABEL: &str = "com.ressource-utility";

fn plist_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| {
        h.join("Library/LaunchAgents")
            .join(format!("{LABEL}.plist"))
    })
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn plist_contents(exe: &Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <false/>
</dict>
</plist>
"#,
        xml_escape(&exe.to_string_lossy())
    )
}

pub fn is_enabled() -> bool {
    plist_path().is_some_and(|p| p.exists())
}

pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let path = plist_path().ok_or("Dossier personnel introuvable")?;
    if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, plist_contents(&exe)).map_err(|e| e.to_string())
    } else {
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_points_to_executable() {
        let p = plist_contents(Path::new(
            "/Apps/R & U.app/Contents/MacOS/ressource-utility",
        ));
        assert!(
            p.contains("<string>/Apps/R &amp; U.app/Contents/MacOS/ressource-utility</string>")
        );
        assert!(p.contains("<key>RunAtLoad</key>\n    <true/>"));
        assert!(p.contains("<key>KeepAlive</key>\n    <false/>"));
    }
}
