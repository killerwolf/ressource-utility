//! Base de connaissances locale : verdict + explication en français simple,
//! sans aucun appel au LLM.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Normal,
    Watch,
    Abnormal,
    Unknown,
}

impl Verdict {
    pub fn emoji(self) -> &'static str {
        match self {
            Verdict::Normal => "✅",
            Verdict::Watch => "👀",
            Verdict::Abnormal => "🛑",
            Verdict::Unknown => "❓",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Verdict::Normal => "Normal",
            Verdict::Watch => "À surveiller",
            Verdict::Abnormal => "Anormal",
            Verdict::Unknown => "Inconnu",
        }
    }

    /// Gravité pour comparer deux verdicts (l'inconnu compte comme normal).
    pub fn severity(self) -> u8 {
        match self {
            Verdict::Normal | Verdict::Unknown => 0,
            Verdict::Watch => 1,
            Verdict::Abnormal => 2,
        }
    }

    /// Verdict le plus grave des deux.
    pub fn max(self, other: Verdict) -> Verdict {
        if other.severity() > self.severity() {
            other
        } else {
            self
        }
    }

    /// Lecture tolérante (accents, casse, décorations).
    pub fn parse(s: &str) -> Option<Verdict> {
        let t: String = s
            .to_lowercase()
            .chars()
            .map(|c| match c {
                'é' | 'è' | 'ê' => 'e',
                'à' | 'â' => 'a',
                c => c,
            })
            .filter(|c| c.is_alphabetic())
            .collect();
        if t.contains("anormal") || t.contains("suspect") {
            Some(Verdict::Abnormal)
        } else if t.contains("surveill") || t.contains("watch") {
            Some(Verdict::Watch)
        } else if t.contains("normal") {
            Some(Verdict::Normal)
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Known {
    pub verdict: Verdict,
    pub text: String,
    /// Verdict issu de la couche sécurité (affiché « Suspect »).
    pub security: bool,
}

/// Retire ` ×N` et le suffixe entre parenthèses ; garde la casse.
pub fn strip_decor(name: &str) -> &str {
    let mut s = name.trim();
    if let Some(i) = s.rfind(" ×")
        && s[i + " ×".len()..].chars().all(|c| c.is_ascii_digit())
    {
        s = &s[..i];
    }
    if let Some(i) = s.find(" (") {
        s = &s[..i];
    }
    s.trim()
}

/// Clé de comparaison : sans décoration, en minuscules.
pub fn normalize(name: &str) -> String {
    strip_decor(name).to_lowercase()
}

const SYS: &[(&str, &str)] = &[
    (
        "avconferenced",
        "Gère les appels audio et vidéo (FaceTime, Teams, Zoom…). Normal pendant un appel.",
    ),
    (
        "callservicesd",
        "Gère les appels FaceTime et ceux de ton iPhone sur le Mac.",
    ),
    (
        "windowserver",
        "Dessine tout ce qui s'affiche à l'écran. Il monte avec plusieurs écrans ou beaucoup de fenêtres.",
    ),
    (
        "kernel_task",
        "Cœur du système. Il monte exprès pour faire baisser la chaleur : normal si le Mac chauffe.",
    ),
    (
        "mds",
        "Spotlight (la recherche) indexe tes fichiers. Temporaire, ça se calme tout seul.",
    ),
    (
        "mds_stores",
        "Spotlight (la recherche) indexe tes fichiers. Temporaire, ça se calme tout seul.",
    ),
    (
        "mdworker",
        "Spotlight (la recherche) indexe tes fichiers. Temporaire, ça se calme tout seul.",
    ),
    (
        "mdworker_shared",
        "Spotlight (la recherche) indexe tes fichiers. Temporaire, ça se calme tout seul.",
    ),
    (
        "corespotlightd",
        "Aide Spotlight à retrouver tes documents et tes mails.",
    ),
    (
        "photoanalysisd",
        "Analyse tes photos (visages, objets). Normal après un import, ça finit seul.",
    ),
    (
        "mediaanalysisd",
        "Analyse tes photos et vidéos pour l'app Photos. Temporaire.",
    ),
    (
        "backupd",
        "Time Machine fait une sauvegarde. Laisse-le finir.",
    ),
    (
        "cloudd",
        "Synchronise iCloud. Normal, surtout après beaucoup de changements.",
    ),
    ("bird", "Synchronise iCloud Drive."),
    (
        "fileproviderd",
        "Synchronise tes fichiers iCloud, Dropbox ou Drive.",
    ),
    ("softwareupdated", "Gère les mises à jour de macOS."),
    (
        "nsurlsessiond",
        "Télécharge en arrière-plan (mises à jour, iCloud, applications).",
    ),
    ("coreaudiod", "Gère le son du Mac."),
    ("bluetoothd", "Gère le Bluetooth (souris, casque, clavier)."),
    (
        "trustd",
        "Vérifie les certificats de sécurité des sites et des apps.",
    ),
    (
        "launchd",
        "Premier programme du Mac : il lance et surveille tous les autres.",
    ),
    ("finder", "Le Finder, l'explorateur de fichiers du Mac."),
    ("dock", "Le Dock, la barre d'applications."),
    ("spotlight", "La recherche Spotlight."),
    (
        "sharingd",
        "Gère AirDrop et le partage entre appareils Apple.",
    ),
    (
        "rapportd",
        "Fait fonctionner la continuité entre tes appareils Apple.",
    ),
    (
        "syspolicyd",
        "Vérifie que les applications que tu ouvres sont sûres.",
    ),
    ("xprotectservice", "L'antivirus intégré d'Apple (XProtect)."),
    ("xprotect", "L'antivirus intégré d'Apple."),
    (
        "deleted",
        "Fait le ménage dans l'espace disque géré par le système.",
    ),
    (
        "triald",
        "Service Apple de tests et d'expériences. Sans danger.",
    ),
    (
        "biomeagent",
        "Service Apple qui alimente les suggestions de Siri et de Spotlight. Normal.",
    ),
    (
        "spotlightknowledged",
        "Service Apple de Spotlight (suggestions et recherche). Normal.",
    ),
    (
        "spotlightknowledged.updater",
        "Met à jour les connaissances de Spotlight. Normal et temporaire.",
    ),
    ("usernoted", "Affiche les notifications."),
    ("notificationcenter", "Le centre de notifications."),
    (
        "cfprefsd",
        "Lit et enregistre les préférences des applications.",
    ),
    ("loginwindow", "Gère ta session et l'écran de connexion."),
    (
        "systemuiserver",
        "Gère la barre des menus en haut de l'écran.",
    ),
    (
        "controlcenter",
        "Le centre de contrôle de la barre des menus.",
    ),
    ("airportd", "Gère la connexion Wi-Fi."),
    ("powerd", "Gère l'énergie et la mise en veille."),
    ("locationd", "Gère la localisation du Mac."),
    ("apsd", "Reçoit les notifications push d'Apple."),
    (
        "identityservicesd",
        "Fait fonctionner iMessage et FaceTime.",
    ),
    ("imagent", "Fait fonctionner Messages."),
    (
        "tccd",
        "Gère les autorisations des apps (caméra, micro, fichiers).",
    ),
    ("logd", "Enregistre le journal du système."),
    ("hidd", "Gère le clavier, la souris et le trackpad."),
    (
        "distnoted",
        "Fait circuler des messages entre les applications.",
    ),
    ("accountsd", "Gère tes comptes en ligne (iCloud, mail…)."),
    (
        "suggestd",
        "Prépare les suggestions de Spotlight et de Siri.",
    ),
    ("assistantd", "Fait fonctionner Siri."),
    ("diskarbitrationd", "Gère les disques et clés USB branchés."),
    (
        "windowmanager",
        "Gère l'organisation des fenêtres (Stage Manager).",
    ),
    ("safari", "Le navigateur Safari."),
    ("mail", "L'application Mail."),
    ("messages", "L'application Messages."),
    ("photos", "L'application Photos."),
    ("google chrome", "Le navigateur Google Chrome."),
    ("brave browser", "Le navigateur Brave."),
    ("microsoft edge", "Le navigateur Microsoft Edge."),
    ("firefox", "Le navigateur Firefox."),
    (
        "plugin-container",
        "Un onglet de Firefox. Normal, beaucoup de RAM = trop d'onglets.",
    ),
    ("claude", "L'application Claude."),
    (
        "ressource-utility",
        "Cette application, qui surveille ton Mac. Elle est très légère.",
    ),
    ("activity monitor", "Le Moniteur d'activité de macOS."),
    ("terminal", "L'application Terminal."),
];

const BROWSERS: &[&str] = &[
    "google chrome",
    "brave browser",
    "microsoft edge",
    "vivaldi",
    "opera",
    "arc",
    "chromium",
];

fn k(verdict: Verdict, text: impl Into<String>) -> Option<Known> {
    Some(Known {
        verdict,
        text: text.into(),
        security: false,
    })
}

/// Cherche un processus dans la base locale.
pub fn lookup(name: &str) -> Option<Known> {
    let key = normalize(name);
    if let Some((_, text)) = SYS.iter().find(|(n, _)| *n == key) {
        return k(Verdict::Normal, *text);
    }
    if key.starts_with("com.apple.webkit.") {
        return k(
            Verdict::Normal,
            "Un onglet Safari ou une page web affichée dans une app. Normal ; beaucoup de RAM = trop d'onglets.",
        );
    }
    if key.starts_with("com.apple.") {
        return k(Verdict::Normal, "Un service du système macOS. Normal.");
    }
    if key.ends_with(" helper") || key == "helper" {
        // « Google Chrome Helper », « Slack Helper (GPU) », « Code Helper (Renderer) »…
        let base = strip_decor(name);
        let app = base[..base.len().saturating_sub(" Helper".len())].trim();
        let app_key = app.to_lowercase();
        if BROWSERS.contains(&app_key.as_str()) {
            return k(
                Verdict::Normal,
                format!(
                    "Un onglet ou une extension de {app}. Normal, mais beaucoup de RAM = trop d'onglets ouverts."
                ),
            );
        }
        if !app.is_empty() {
            return k(
                Verdict::Normal,
                format!("Partie de l'application {app} (processus de fond). Normal."),
            );
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_names() {
        assert_eq!(
            normalize("Google Chrome Helper (Renderer) ×12"),
            "google chrome helper"
        );
        assert_eq!(normalize("avconferenced ×2"), "avconferenced");
        assert_eq!(normalize("  Dock "), "dock");
        assert_eq!(strip_decor("Claude Helper (GPU)"), "Claude Helper");
    }

    #[test]
    fn matches_system_processes() {
        let k = lookup("avconferenced").unwrap();
        assert_eq!(k.verdict, Verdict::Normal);
        assert!(k.text.contains("appels"));
        assert!(lookup("KERNEL_TASK").unwrap().text.contains("chaleur"));
        assert!(
            lookup("com.apple.WebKit.WebContent")
                .unwrap()
                .text
                .contains("Safari")
        );
        assert!(lookup("com.apple.foo.bar").is_some());
        assert!(lookup("mystery_daemon").is_none());
        assert!(lookup("Helper2000").is_none());
    }

    #[test]
    fn matches_browser_and_electron_helpers() {
        let c = lookup("Google Chrome Helper (Aperitif Renderer) ×24").unwrap();
        assert!(c.text.contains("Google Chrome") && c.text.contains("onglet"));
        assert!(
            lookup("Brave Browser Helper (GPU)")
                .unwrap()
                .text
                .contains("Brave Browser")
        );
        let e = lookup("Slack Helper").unwrap();
        assert!(e.text.contains("l'application Slack"));
        assert!(
            lookup("Code Helper (Renderer)")
                .unwrap()
                .text
                .contains("Code")
        );
    }

    #[test]
    fn verdict_parsing_is_lenient() {
        assert_eq!(Verdict::parse("normal"), Some(Verdict::Normal));
        assert_eq!(Verdict::parse(" **Anormal** "), Some(Verdict::Abnormal));
        assert_eq!(Verdict::parse("À surveiller"), Some(Verdict::Watch));
        assert_eq!(Verdict::parse("surveiller"), Some(Verdict::Watch));
        assert_eq!(Verdict::parse("blah"), None);
    }
}
