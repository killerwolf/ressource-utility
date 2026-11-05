//! Couche sécurité 100 % locale (zéro jeton) : mineurs de cryptomonnaie,
//! usurpation de noms système, emplacements suspects, signature de code,
//! éléments de démarrage automatique. Les vérifications coûteuses ne portent
//! que sur quelques candidats et sont mises en cache (chemin + date de modification).

use crate::known::{self, Known, Verdict};
use crate::metrics::{ProcInfo, ProcTop};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, UNIX_EPOCH};

const HOT_CPU: f32 = 30.0;
const HOT_SCANS: u32 = 3;
const IDLE_SECS: u64 = 120;

// ---------------------------------------------------------------------------
// Signaux purs
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Weak,
    Medium,
    Strong,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Signal {
    pub level: Level,
    pub reason: String,
}

fn sig(level: Level, reason: impl Into<String>) -> Signal {
    Signal {
        level,
        reason: reason.into(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trust {
    Apple,
    DeveloperId,
    AdHoc,
    Unsigned,
    Unknown,
}

impl Trust {
    fn trusted(self) -> bool {
        matches!(self, Trust::Apple | Trust::DeveloperId)
    }
}

const MINER_NAMES: &[&str] = &[
    "xmrig",
    "xmr-stak",
    "minerd",
    "cpuminer",
    "cgminer",
    "bfgminer",
    "ethminer",
    "lolminer",
    "nbminer",
    "t-rex",
    "phoenixminer",
    "gminer",
    "teamredminer",
    "nanominer",
    "srbminer",
    "kinsing",
    "kdevtmpfsi",
    "nicehash",
];

/// Nom de mineur / malware connu (insensible à la casse, sous-chaîne).
pub fn miner_name(name: &str) -> Option<&'static str> {
    let n = name.to_lowercase();
    MINER_NAMES.iter().copied().find(|m| n.contains(m))
}

/// Binaires système dont le nom est souvent usurpé.
const SYSTEM_BINARIES: &[&str] = &[
    "kernel_task",
    "windowserver",
    "launchd",
    "mds",
    "mds_stores",
    "mdworker",
    "mdworker_shared",
    "finder",
    "dock",
    "loginwindow",
    "syspolicyd",
    "trustd",
    "cfprefsd",
    "coreaudiod",
    "bluetoothd",
    "backupd",
    "cloudd",
    "bird",
    "nsurlsessiond",
    "softwareupdated",
    "sharingd",
    "rapportd",
    "xprotectservice",
    "powerd",
    "locationd",
    "tccd",
    "logd",
    "hidd",
    "systemuiserver",
    "controlcenter",
    "usernoted",
    "notificationcenter",
    "airportd",
    "apsd",
    "accountsd",
    "avconferenced",
    "callservicesd",
    "corespotlightd",
    "fileproviderd",
    "photoanalysisd",
    "mediaanalysisd",
    "identityservicesd",
    "imagent",
    "distnoted",
];

pub fn is_system_path(path: &str) -> bool {
    ["/System/", "/usr/", "/Library/Apple/", "/sbin/", "/bin/"]
        .iter()
        .any(|p| path.starts_with(p))
}

/// Un nom de binaire système sans être dans un dossier système.
pub fn impostor(name: &str, path: &str) -> bool {
    SYSTEM_BINARIES.contains(&known::normalize(name).as_str()) && !is_system_path(path)
}

/// Pour « X Helper » : le programme doit être dans une application `.app` de X.
/// `true` = l'aide est hors d'une application correspondante (douteux).
pub fn helper_outside_bundle(name: &str, path: &str) -> bool {
    let base = known::strip_decor(name);
    let lower = base.to_lowercase();
    let Some(app) = lower.strip_suffix(" helper") else {
        return false;
    };
    let p = path.to_lowercase();
    !(p.contains(".app/") && p.contains(app.trim()))
}

const DEV_DIRS: &[&str] = &[
    ".cargo",
    ".rustup",
    ".nvm",
    ".local",
    ".pyenv",
    ".volta",
    ".bun",
    ".deno",
    ".npm",
    ".vscode",
    ".cursor",
    ".claude",
    ".docker",
    ".sdkman",
    ".asdf",
    ".gem",
    ".rbenv",
    ".pnpm",
    ".antigravity",
    ".gradle",
    ".m2",
    ".orbstack",
    ".colima",
    ".lima",
    ".swiftpm",
];

/// Emplacement suspect d'un exécutable.
pub fn classify_path(path: &str, home: &str) -> Option<Signal> {
    for t in ["/tmp/", "/private/tmp/", "/private/var/tmp/", "/var/tmp/"] {
        if path.starts_with(t) {
            return Some(sig(
                Level::Medium,
                "lancé depuis un dossier temporaire (/tmp)",
            ));
        }
    }
    if path.starts_with("/Users/Shared/") {
        return Some(sig(Level::Medium, "lancé depuis /Users/Shared"));
    }
    if !home.is_empty() {
        if path.starts_with(&format!("{home}/Downloads/")) {
            return Some(sig(
                Level::Medium,
                "lancé depuis le dossier Téléchargements",
            ));
        }
        if path.starts_with(&format!("{home}/Library/Caches/")) {
            return Some(sig(Level::Medium, "lancé depuis un dossier de cache"));
        }
    }
    let rel = if !home.is_empty() && path.starts_with(home) {
        &path[home.len()..]
    } else {
        path
    };
    for (i, comp) in rel.split('/').enumerate() {
        if comp.len() > 1 && comp.starts_with('.') && comp != ".." {
            // Outils de développement connus : dossier caché normal sous le dossier perso.
            if i <= 1 && DEV_DIRS.contains(&comp) {
                return None;
            }
            return Some(sig(
                Level::Medium,
                format!("lancé depuis un dossier caché ({comp})"),
            ));
        }
    }
    let var = path.strip_prefix("/private").unwrap_or(path);
    if var.starts_with("/var/folders/") && var.contains("/T/") {
        return Some(sig(
            Level::Weak,
            "lancé depuis un dossier temporaire du système",
        ));
    }
    None
}

/// Analyse la sortie (stderr) de `codesign -dv --verbose=2`.
pub fn parse_codesign(output: &str) -> Trust {
    if output.contains("not signed at all") {
        Trust::Unsigned
    } else if output.contains("Authority=Software Signing") {
        Trust::Apple
    } else if [
        "Developer ID Application",
        "Apple Development",
        "Apple Mac OS Application Signing",
        "3rd Party Mac Developer",
        "Apple Distribution",
    ]
    .iter()
    .any(|a| output.contains(&format!("Authority={a}")))
    {
        Trust::DeveloperId
    } else if output.contains("Signature=adhoc")
        || output.contains("(adhoc")
        || output.contains(",adhoc")
    {
        Trust::AdHoc
    } else {
        Trust::Unknown
    }
}

pub fn trust_label(t: Trust) -> Option<&'static str> {
    match t {
        Trust::Apple => Some("signé par Apple"),
        Trust::DeveloperId => Some("signé par un développeur identifié"),
        Trust::AdHoc => Some("signature locale seulement"),
        Trust::Unsigned => Some("non signé"),
        Trust::Unknown => None,
    }
}

fn trust_signal(trust: Trust, path: &str) -> Option<Signal> {
    let managed = path.starts_with("/opt/homebrew/") || path.starts_with("/usr/local/");
    let level = if managed { Level::Weak } else { Level::Medium };
    match trust {
        Trust::Unsigned => Some(sig(level, "non signé")),
        Trust::AdHoc => Some(sig(level, "signature locale seulement (non vérifiable)")),
        _ => None,
    }
}

/// Verdict : un signal fort, ou deux signaux moyens => suspect ; un moyen => à surveiller.
pub fn score(signals: &[Signal]) -> Verdict {
    let medium = signals.iter().filter(|s| s.level == Level::Medium).count();
    if signals.iter().any(|s| s.level == Level::Strong) || medium >= 2 {
        Verdict::Abnormal
    } else if medium == 1 {
        Verdict::Watch
    } else {
        Verdict::Normal
    }
}

/// Durée d'inactivité (secondes) lue dans la sortie de `ioreg -c IOHIDSystem`.
pub fn parse_hid_idle(ioreg: &str) -> Option<u64> {
    let rest = &ioreg[ioreg.find("\"HIDIdleTime\" = ")? + "\"HIDIdleTime\" = ".len()..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse::<u64>().ok().map(|ns| ns / 1_000_000_000)
}

/// Remplace uniquement le dossier personnel par `~`.
pub fn sanitize_path(path: &str, home: &str) -> String {
    if !home.is_empty()
        && let Some(rest) = path.strip_prefix(home)
        && (rest.is_empty() || rest.starts_with('/'))
    {
        return format!("~{rest}");
    }
    path.to_string()
}

pub struct LaunchItem {
    pub label: String,
    /// Exécutable effectivement lancé (le script visé pour les interpréteurs).
    pub target: String,
}

/// Lit un plist launchd converti en JSON.
pub fn parse_launchd_json(json: &str) -> Option<LaunchItem> {
    let v: Value = serde_json::from_str(json).ok()?;
    let label = v.get("Label")?.as_str()?.to_string();
    let args: Vec<&str> = v
        .get("ProgramArguments")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let program = v
        .get("Program")
        .and_then(Value::as_str)
        .or_else(|| args.first().copied())?;
    let base = program.rsplit('/').next().unwrap_or(program);
    let interpreters = [
        "sh",
        "bash",
        "zsh",
        "python",
        "python3",
        "osascript",
        "perl",
        "ruby",
        "node",
        "env",
    ];
    let target = if interpreters.contains(&base) {
        args.iter()
            .skip(1)
            .find(|a| a.starts_with('/'))
            .copied()
            .unwrap_or(program)
    } else {
        program
    };
    Some(LaunchItem {
        label,
        target: target.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Résultats
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Finding {
    pub name: String,
    pub pid: Option<u32>,
    pub path: Option<String>,
    pub verdict: Verdict,
    pub reasons: Vec<String>,
    pub miner: bool,
    /// État de la signature de code, pour l'IA et le bilan.
    pub signature: Option<&'static str>,
    /// Avis de l'IA (ne peut qu'aggraver le verdict local).
    pub ai: Option<String>,
    /// Fichier .plist pour un élément de démarrage automatique.
    pub startup: Option<String>,
}

impl Finding {
    /// Intègre l'avis de l'IA : verdict final = max(local, IA). Un verdict
    /// local grave n'est jamais abaissé.
    pub fn apply_ai(&mut self, verdict: Verdict, text: &str) {
        self.verdict = self.verdict.max(verdict);
        self.ai = Some(text.to_string());
    }

    /// Phrase simple pour le bilan.
    pub fn text(&self) -> String {
        let r = self.reasons.join(", ");
        if self.miner {
            format!("ressemble à un mineur de cryptomonnaie : {r}")
        } else {
            r
        }
    }

    /// Clé de dédoublonnage des notifications.
    pub fn key(&self) -> String {
        self.path.clone().unwrap_or_else(|| self.name.clone())
    }
}

pub fn judged_known(f: &Finding, home: &str) -> Known {
    let loc = f
        .path
        .as_deref()
        .map(|p| format!(" ({})", sanitize_path(p, home)))
        .unwrap_or_default();
    Known {
        verdict: f.verdict,
        text: format!("{}{loc}", capitalize(&f.text())),
        security: true,
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Système (appels coûteux, réservés aux candidats)
// ---------------------------------------------------------------------------

pub fn home() -> String {
    dirs::home_dir()
        .map(|h| h.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Chemin de l'exécutable d'un PID (appel système léger).
pub fn pid_path(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    let mut buf = vec![0u8; 4096];
    // SAFETY: tampon valide de la taille annoncée.
    let n = unsafe { libc::proc_pidpath(pid as i32, buf.as_mut_ptr().cast(), buf.len() as u32) };
    if n <= 0 {
        return None;
    }
    buf.truncate(n as usize);
    String::from_utf8(buf).ok()
}

fn mtime(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn hid_idle_secs() -> Option<u64> {
    let out = Command::new("/usr/sbin/ioreg")
        .args(["-c", "IOHIDSystem"])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    parse_hid_idle(&String::from_utf8_lossy(&out.stdout))
}

fn run_codesign(path: &str) -> Trust {
    Command::new("/usr/bin/codesign")
        .args(["-dv", "--verbose=2", path])
        .stdin(Stdio::null())
        .output()
        .map(|o| parse_codesign(&String::from_utf8_lossy(&o.stderr)))
        .unwrap_or(Trust::Unknown)
}

// ---------------------------------------------------------------------------
// Auditeur
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct Auditor {
    trust_cache: HashMap<(String, u64), Trust>,
    hot: HashMap<String, (u32, Instant)>,
    startup_cache: HashMap<(PathBuf, u64), Option<Finding>>,
    own_exe: Option<String>,
    home: String,
}

pub struct ScanOutput {
    pub findings: Vec<Finding>,
    /// Lignes de diagnostic (pour `--security`).
    pub inspected: Vec<String>,
}

fn fmt_duration(d: Duration) -> String {
    let s = d.as_secs();
    if s < 90 {
        format!("{s} s")
    } else {
        format!("{} min", s / 60)
    }
}

impl Auditor {
    pub fn new() -> Self {
        Self {
            own_exe: std::env::current_exe()
                .ok()
                .map(|p| p.to_string_lossy().into_owned()),
            home: home(),
            ..Default::default()
        }
    }

    fn trust(&mut self, path: &str) -> Trust {
        let key = (path.to_string(), mtime(Path::new(path)));
        if let Some(t) = self.trust_cache.get(&key) {
            return *t;
        }
        let t = run_codesign(path);
        self.trust_cache.insert(key, t);
        t
    }

    fn update_hot(&mut self, top: &ProcTop) {
        let now = Instant::now();
        let mut seen = Vec::new();
        for p in &top.by_cpu {
            let key = known::normalize(&p.name);
            if p.cpu >= HOT_CPU {
                let e = self.hot.entry(key.clone()).or_insert((0, now));
                e.0 += 1;
                seen.push(key);
            }
        }
        self.hot.retain(|k, _| seen.contains(k));
    }

    /// Signaux liés à un exécutable (hors nom).
    fn path_signals(&mut self, path: &str, needs_signature: bool) -> (Vec<Signal>, Trust) {
        let mut out = Vec::new();
        let trust = if needs_signature && !is_system_path(path) {
            self.trust(path)
        } else {
            Trust::Unknown
        };
        if let Some(mut s) = classify_path(path, &self.home) {
            if trust.trusted() && s.level == Level::Medium {
                s.level = Level::Weak;
            }
            out.push(s);
        }
        if let Some(s) = trust_signal(trust, path) {
            out.push(s);
        }
        (out, trust)
    }

    pub fn scan_processes(&mut self, top: &ProcTop) -> ScanOutput {
        self.update_hot(top);
        let mut findings = Vec::new();
        let mut inspected = Vec::new();
        let mut idle: Option<Option<u64>> = None;

        let mut cands: Vec<&ProcInfo> = crate::report::rows(top);
        for p in &top.extra {
            if !cands.iter().any(|q| q.name == p.name) {
                cands.push(p);
            }
        }
        for p in cands {
            let name = known::strip_decor(&p.name).to_string();
            let key = known::normalize(&p.name);
            let kb = known::lookup(&p.name);
            let hot = self.hot.get(&key).copied().filter(|(n, _)| *n >= HOT_SCANS);
            let miner = miner_name(&name);
            let path = pid_path(p.pid);
            if path.is_some() && path == self.own_exe {
                continue;
            }
            let is_helper = key.ends_with(" helper");
            let is_sysname = SYSTEM_BINARIES.contains(&key.as_str());
            let candidate =
                miner.is_some() || kb.is_none() || hot.is_some() || is_helper || is_sysname;
            if !candidate {
                continue;
            }
            let mut signals: Vec<Signal> = Vec::new();
            let mut behaviour = false;
            let mut trust = Trust::Unknown;
            if let Some(m) = miner {
                signals.push(sig(
                    Level::Strong,
                    format!("nom connu de mineur de cryptomonnaie ({m})"),
                ));
            }
            if let Some(path) = &path {
                if impostor(&name, path) {
                    signals.push(sig(
                        Level::Strong,
                        "porte le nom d'un processus macOS mais ne se trouve pas dans le système",
                    ));
                }
                if is_helper && helper_outside_bundle(&p.name, path) {
                    signals.push(sig(
                        Level::Medium,
                        "se présente comme l'aide d'une application mais n'est pas dans cette application",
                    ));
                }
                let needs_sig = kb.is_none() || hot.is_some() || miner.is_some();
                let (sigs, t) = self.path_signals(path, needs_sig);
                trust = t;
                signals.extend(sigs);
                if hot.is_some()
                    && kb.is_none()
                    && signals.iter().any(|s| s.level == Level::Medium)
                    && *idle
                        .get_or_insert_with(hid_idle_secs)
                        .as_ref()
                        .unwrap_or(&0)
                        >= IDLE_SECS
                {
                    behaviour = true;
                    signals.push(sig(
                        Level::Medium,
                        "tourne à plein régime alors que tu n'utilises pas le Mac",
                    ));
                }
            }
            let verdict = score(&signals);
            inspected.push(format!(
                "{name} (pid {}) chemin={} chaud={} signaux={:?} => {}",
                p.pid,
                path.as_deref().unwrap_or("illisible"),
                hot.is_some(),
                signals
                    .iter()
                    .map(|s| (&s.level, s.reason.as_str()))
                    .collect::<Vec<_>>(),
                verdict.label()
            ));
            if verdict == Verdict::Normal {
                continue;
            }
            let mut reasons: Vec<String> = signals
                .iter()
                .filter(|s| s.level >= Level::Medium)
                .map(|s| s.reason.clone())
                .collect();
            if let Some((_, since)) = hot {
                reasons.insert(
                    0,
                    format!(
                        "CPU {:.0} % depuis {}",
                        p.cpu,
                        fmt_duration(since.elapsed())
                    ),
                );
            }
            findings.push(Finding {
                name,
                pid: Some(p.pid),
                path,
                verdict,
                reasons,
                miner: miner.is_some() || behaviour,
                signature: trust_label(trust),
                ai: None,
                startup: None,
            });
        }
        ScanOutput {
            findings,
            inspected,
        }
    }

    /// Audit des éléments de démarrage automatique (mis en cache par date).
    pub fn audit_startup(&mut self) -> ScanOutput {
        let mut dirs: Vec<PathBuf> = vec![
            "/Library/LaunchAgents".into(),
            "/Library/LaunchDaemons".into(),
        ];
        if let Some(h) = dirs::home_dir() {
            dirs.push(h.join("Library/LaunchAgents"));
        }
        let mut findings = Vec::new();
        let mut inspected = Vec::new();
        for dir in dirs {
            let Ok(rd) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in rd.flatten() {
                let path = e.path();
                if path.extension().and_then(|x| x.to_str()) != Some("plist") {
                    continue;
                }
                let key = (path.clone(), mtime(&path));
                let res = match self.startup_cache.get(&key) {
                    Some(r) => r.clone(),
                    None => {
                        let r = self.audit_plist(&path);
                        self.startup_cache.insert(key, r.clone());
                        r
                    }
                };
                inspected.push(format!(
                    "{} => {}",
                    path.display(),
                    res.as_ref()
                        .map_or("rien à signaler", |f| f.verdict.label())
                ));
                findings.extend(res);
            }
        }
        ScanOutput {
            findings,
            inspected,
        }
    }

    fn audit_plist(&mut self, plist: &Path) -> Option<Finding> {
        let out = Command::new("/usr/bin/plutil")
            .args(["-convert", "json", "-o", "-"])
            .arg(plist)
            .stdin(Stdio::null())
            .output()
            .ok()?;
        let item = parse_launchd_json(&String::from_utf8_lossy(&out.stdout))?;
        if item.label.starts_with("com.ressource-utility") {
            return None;
        }
        if !Path::new(&item.target).exists() {
            return None;
        }
        let mut signals = Vec::new();
        let base = item.target.rsplit('/').next().unwrap_or(&item.target);
        if let Some(m) = miner_name(base).or_else(|| miner_name(&item.label)) {
            signals.push(sig(
                Level::Strong,
                format!("nom connu de mineur de cryptomonnaie ({m})"),
            ));
        }
        let (sigs, trust) = self.path_signals(&item.target, true);
        signals.extend(sigs);
        let verdict = score(&signals);
        if verdict == Verdict::Normal {
            return None;
        }
        let miner = signals.iter().any(|s| s.reason.contains("mineur"));
        Some(Finding {
            name: plist
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            pid: None,
            path: Some(item.target),
            verdict,
            reasons: signals
                .into_iter()
                .filter(|s| s.level >= Level::Medium)
                .map(|s| s.reason)
                .collect(),
            miner,
            signature: trust_label(trust),
            ai: None,
            startup: Some(plist.to_string_lossy().into_owned()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/Users/jo";

    #[test]
    fn matches_miner_names() {
        assert_eq!(miner_name("XMRig"), Some("xmrig"));
        assert_eq!(miner_name("kdevtmpfsd"), None);
        assert_eq!(miner_name("kdevtmpfsi"), Some("kdevtmpfsi"));
        assert_eq!(miner_name("t-rex"), Some("t-rex"));
        assert_eq!(miner_name("Safari"), None);
        assert_eq!(miner_name("com.apple.mdworker"), None);
    }

    #[test]
    fn classifies_paths() {
        let lvl = |p: &str| classify_path(p, HOME).map(|s| s.level);
        assert_eq!(lvl("/tmp/x/run"), Some(Level::Medium));
        assert_eq!(lvl("/private/var/tmp/a"), Some(Level::Medium));
        assert_eq!(lvl("/Users/jo/Downloads/app"), Some(Level::Medium));
        assert_eq!(lvl("/Users/jo/Library/Caches/x/y"), Some(Level::Medium));
        assert_eq!(lvl("/Users/Shared/y"), Some(Level::Medium));
        assert_eq!(lvl("/Users/jo/.hidden/run"), Some(Level::Medium));
        assert_eq!(lvl("/Users/jo/.cargo/bin/cargo"), None);
        assert_eq!(
            lvl("/private/var/folders/ab/cd/T/updater"),
            Some(Level::Weak)
        );
        assert_eq!(lvl("/Applications/Foo.app/Contents/MacOS/Foo"), None);
        assert_eq!(lvl("/usr/bin/ls"), None);
    }

    #[test]
    fn parses_codesign_output() {
        let apple = "Executable=/bin/ls\nIdentifier=com.apple.ls\nAuthority=Software Signing\nAuthority=Apple Code Signing Certification Authority\nAuthority=Apple Root CA\n";
        assert_eq!(parse_codesign(apple), Trust::Apple);
        let dev = "Identifier=com.foo\nAuthority=Developer ID Application: Foo Inc (ABCDE12345)\nAuthority=Developer ID Certification Authority\nAuthority=Apple Root CA\n";
        assert_eq!(parse_codesign(dev), Trust::DeveloperId);
        let adhoc = "Identifier=a\nCodeDirectory v=20400 size=1 flags=0x20002(adhoc,linker-signed) hashes=3\nSignature=adhoc\n";
        assert_eq!(parse_codesign(adhoc), Trust::AdHoc);
        assert_eq!(
            parse_codesign("/tmp/x: code object is not signed at all\n"),
            Trust::Unsigned
        );
        assert_eq!(parse_codesign(""), Trust::Unknown);
    }

    #[test]
    fn detects_impostors() {
        assert!(impostor("kernel_task", "/tmp/kernel_task"));
        assert!(impostor("WindowServer", "/Users/jo/.x/WindowServer"));
        assert!(!impostor("launchd", "/sbin/launchd"));
        assert!(!impostor(
            "Finder",
            "/System/Library/CoreServices/Finder.app/Contents/MacOS/Finder"
        ));
        assert!(!impostor("someapp", "/tmp/someapp"));
    }

    #[test]
    fn helper_outside_bundle_is_flagged() {
        let chrome = "/Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Framework.framework/Helpers/Google Chrome Helper.app/Contents/MacOS/Google Chrome Helper";
        assert!(!helper_outside_bundle(
            "Google Chrome Helper (Renderer)",
            chrome
        ));
        assert!(helper_outside_bundle(
            "Google Chrome Helper",
            "/tmp/Google Chrome Helper"
        ));
        assert!(helper_outside_bundle(
            "Slack Helper",
            "/Users/jo/Library/Caches/Foo.app/x"
        ));
        let code = "/Applications/Visual Studio Code.app/Contents/Frameworks/Code Helper.app/Contents/MacOS/Code Helper";
        assert!(!helper_outside_bundle("Code Helper (Renderer)", code));
        assert!(!helper_outside_bundle("Safari", "/tmp/x"));
    }

    #[test]
    fn scoring() {
        let m = || sig(Level::Medium, "m");
        assert_eq!(score(&[]), Verdict::Normal);
        assert_eq!(
            score(&[sig(Level::Weak, "w"), sig(Level::Weak, "w")]),
            Verdict::Normal
        );
        assert_eq!(score(&[m()]), Verdict::Watch);
        assert_eq!(score(&[m(), m()]), Verdict::Abnormal);
        assert_eq!(score(&[sig(Level::Strong, "s")]), Verdict::Abnormal);
    }

    #[test]
    fn parses_idle_time() {
        let s = "    | |   \"HIDIdleTime\" = 4294967295\n    \"Other\" = 1";
        assert_eq!(parse_hid_idle(s), Some(4));
        let s2 = "\"HIDIdleTime\" = 125000000000";
        assert_eq!(parse_hid_idle(s2), Some(125));
        assert_eq!(parse_hid_idle("rien"), None);
    }

    #[test]
    fn parses_launchd_plist() {
        let j = r#"{"Label":"com.foo.run","ProgramArguments":["/Users/jo/.hidden/run","--x"],"RunAtLoad":true}"#;
        let i = parse_launchd_json(j).unwrap();
        assert_eq!(
            (i.label.as_str(), i.target.as_str()),
            ("com.foo.run", "/Users/jo/.hidden/run")
        );
        let j2 = r#"{"Label":"a","ProgramArguments":["/bin/bash","-c","/tmp/x.sh"]}"#;
        assert_eq!(parse_launchd_json(j2).unwrap().target, "/tmp/x.sh");
        let j3 = r#"{"Label":"a","ProgramArguments":["/bin/sh","/tmp/x.sh"]}"#;
        assert_eq!(parse_launchd_json(j3).unwrap().target, "/tmp/x.sh");
        let j4 = r#"{"Label":"b","Program":"/opt/homebrew/bin/foo"}"#;
        assert_eq!(
            parse_launchd_json(j4).unwrap().target,
            "/opt/homebrew/bin/foo"
        );
        assert!(parse_launchd_json("pas du json").is_none());
    }

    #[test]
    fn ai_can_raise_but_never_lower() {
        let mk = |v| Finding {
            name: "x".into(),
            pid: Some(5),
            path: None,
            verdict: v,
            reasons: vec![],
            miner: false,
            signature: None,
            ai: None,
            startup: None,
        };
        // Un 🛑 local reste 🛑 même si l'IA dit « normal » (injection par le nom, etc.).
        let mut f = mk(Verdict::Abnormal);
        f.apply_ai(Verdict::Normal, "ignore");
        assert_eq!(f.verdict, Verdict::Abnormal);
        let mut f = mk(Verdict::Watch);
        f.apply_ai(Verdict::Normal, "ok");
        assert_eq!(f.verdict, Verdict::Watch);
        f.apply_ai(Verdict::Abnormal, "suspect");
        assert_eq!(f.verdict, Verdict::Abnormal);
        assert_eq!(f.ai.as_deref(), Some("suspect"));
        assert_eq!(Verdict::Watch.max(Verdict::Unknown), Verdict::Watch);
        assert_eq!(Verdict::Normal.max(Verdict::Watch), Verdict::Watch);
    }

    #[test]
    fn sanitizes_paths() {
        assert_eq!(sanitize_path("/Users/jo/.x/run", HOME), "~/.x/run");
        assert_eq!(sanitize_path("/Users/john/run", HOME), "/Users/john/run");
        assert_eq!(sanitize_path("/tmp/run", HOME), "/tmp/run");
    }

    #[test]
    fn trust_signals() {
        assert_eq!(
            trust_signal(Trust::Unsigned, "/tmp/x").unwrap().level,
            Level::Medium
        );
        assert_eq!(
            trust_signal(Trust::AdHoc, "/opt/homebrew/bin/x")
                .unwrap()
                .level,
            Level::Weak
        );
        assert!(trust_signal(Trust::DeveloperId, "/tmp/x").is_none());
    }
}
