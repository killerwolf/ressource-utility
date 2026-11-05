//! Ressource Utility — moniteur de ressources dans la barre de menus macOS.

mod anomaly;
mod autostart;
mod config;
mod dialogs;
mod icon;
mod known;
mod llm;
mod menu;
mod metrics;
mod prompt;
mod report;
mod secrets;
mod security;

use anomaly::{Anomaly, Detector, Sample};
use config::{Config, Explanations};
use known::Verdict;
use llm::{LlmError, Provider};
use menu::{Action, AppMenu, one_line};
use metrics::{ProcTop, Refresh, Sampler, Snapshot, fmt_go};
use security::{Auditor, Finding};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
use tray_icon::menu::{MenuEvent, MenuId};
use tray_icon::{TrayIcon, TrayIconBuilder};

const SAMPLE_EVERY: Duration = Duration::from_secs(3);
const SLOW_EVERY: Duration = Duration::from_secs(60);
const PROCS_SUSPECT: Duration = Duration::from_secs(9);
const NOTIFY_DEDUPE: Duration = Duration::from_secs(30 * 60);

enum UserEvent {
    Snapshot(Box<Snapshot>),
    Menu(MenuId),
    Analysis {
        result: Result<prompt::Reply, String>,
        manual: bool,
    },
    ProcAnswer {
        name: String,
        result: Result<(Verdict, String), String>,
        security: bool,
    },
    Security {
        procs: Option<Vec<Finding>>,
        startup: Option<Vec<Finding>>,
    },
    KeyStatus(Provider, bool),
    /// Ok(true) = enregistrée, Ok(false) = annulé.
    KeyEntered(Provider, Result<bool, String>),
    ModelChosen(Provider, String),
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Échantillonnage en arrière-plan
// ---------------------------------------------------------------------------

/// Boucle d'échantillonnage partagée par l'application et `--bench`.
/// `emit` retourne `false` pour arrêter.
fn sample_loop(mut emit: impl FnMut(Snapshot) -> bool) {
    let mut sampler = match Sampler::new() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("sysinfo indisponible : {e}");
            return;
        }
    };
    std::thread::sleep(Duration::from_millis(700));
    let (mut last_procs, mut last_temp, mut last_disk): (Option<Instant>, _, _) =
        (None, None, None);
    let mut suspect = false;
    loop {
        let due = |t: Option<Instant>, every: Duration| t.is_none_or(|t| t.elapsed() >= every);
        let procs_every = if suspect { PROCS_SUSPECT } else { SLOW_EVERY };
        let flags = Refresh {
            procs: due(last_procs, procs_every),
            temp: due(last_temp, if suspect { PROCS_SUSPECT } else { SLOW_EVERY }),
            disk: due(last_disk, SLOW_EVERY),
        };
        let snap = sampler.sample(flags);
        let now = Instant::now();
        if flags.procs {
            last_procs = Some(now);
        }
        if flags.temp {
            last_temp = Some(now);
        }
        if flags.disk {
            last_disk = Some(now);
        }
        suspect = snap.cpu > 70.0 || snap.mem_pct() > 85.0;
        if !emit(snap) {
            break;
        }
        std::thread::sleep(SAMPLE_EVERY);
    }
}

fn spawn_sampler(proxy: EventLoopProxy<UserEvent>) {
    std::thread::spawn(move || {
        sample_loop(|snap| {
            proxy
                .send_event(UserEvent::Snapshot(Box::new(snap)))
                .is_ok()
        });
    });
}

/// `--bench N` : exécute la boucle d'échantillonnage N secondes, sans UI, et
/// affiche le temps CPU consommé (getrusage) et la RSS maximale.
fn bench(secs: u64) {
    let start = Instant::now();
    let cpu = || {
        let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
        // SAFETY: `ru` est une structure valide pour l'appel.
        unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut ru) };
        let t = |tv: libc::timeval| tv.tv_sec as f64 + tv.tv_usec as f64 / 1e6;
        (t(ru.ru_utime) + t(ru.ru_stime), ru.ru_maxrss as f64 / 1e6)
    };
    let (c0, _) = cpu();
    // Comme l'application : analyse des processus à chaque relevé complet,
    // audit du démarrage automatique une fois au début.
    let mut auditor = Auditor::new();
    auditor.audit_startup();
    sample_loop(|snap| {
        if let Some(top) = &snap.procs {
            auditor.scan_processes(top);
        }
        start.elapsed() < Duration::from_secs(secs)
    });
    let (c1, rss) = cpu();
    let wall = start.elapsed().as_secs_f64();
    println!(
        "bench: {:.3} s CPU en {:.1} s ({:.2} %), RSS max {:.1} Mo",
        c1 - c0,
        wall,
        (c1 - c0) * 100.0 / wall,
        rss
    );
}

// ---------------------------------------------------------------------------
// État de l'application (thread principal)
// ---------------------------------------------------------------------------

struct App {
    cfg: Config,
    explanations: Explanations,
    detector: Detector,
    menu: AppMenu,
    tray: TrayIcon,
    proxy: EventLoopProxy<UserEvent>,
    last: Option<Snapshot>,
    top: ProcTop,
    active: Vec<Anomaly>,
    llm_advice: Option<String>,
    error: Option<String>,
    keys: HashMap<Provider, bool>,
    busy: bool,
    pending_procs: HashSet<String>,
    notified: HashMap<String, Instant>,
    title: String,
    title_first: bool,
    auditor: Arc<Mutex<Auditor>>,
    proc_findings: Vec<Finding>,
    startup_findings: Vec<Finding>,
    findings: Vec<Finding>,
    scan_running: bool,
    ai_opinions: HashMap<String, (Verdict, String)>,
    last_startup: Option<Instant>,
}

impl App {
    fn new(proxy: EventLoopProxy<UserEvent>) -> Option<Self> {
        let mut cfg = Config::load();
        cfg.launch_at_login = autostart::is_enabled();
        let menu = AppMenu::build(cfg.provider, cfg.launch_at_login).ok()?;
        let mut builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu.menu.clone()))
            .with_title("…")
            .with_tooltip("Ressource Utility");
        if let Some(icon) = icon::make() {
            builder = builder.with_icon_templated(icon);
        }
        let tray = builder.build().ok()?;

        let p = proxy.clone();
        std::thread::spawn(move || {
            for prov in Provider::ALL {
                let _ = p.send_event(UserEvent::KeyStatus(prov, secrets::get(prov).is_some()));
            }
        });

        let detector = Detector::new(cfg.thresholds.clone());
        Some(Self {
            cfg,
            explanations: Explanations::load(),
            detector,
            menu,
            tray,
            proxy,
            last: None,
            top: ProcTop::default(),
            active: Vec::new(),
            llm_advice: None,
            error: None,
            keys: HashMap::new(),
            busy: false,
            pending_procs: HashSet::new(),
            notified: HashMap::new(),
            title: String::new(),
            title_first: true,
            auditor: Arc::new(Mutex::new(Auditor::new())),
            proc_findings: Vec::new(),
            startup_findings: Vec::new(),
            findings: Vec::new(),
            scan_running: false,
            ai_opinions: HashMap::new(),
            last_startup: None,
        })
    }

    fn has_key(&self) -> bool {
        self.keys.get(&self.cfg.provider).copied().unwrap_or(false)
    }

    fn build_report(&self) -> Option<report::Report> {
        let snap = self.last.as_ref()?;
        Some(report::build(&report::Inputs {
            snap,
            top: &self.top,
            cache: &self.explanations,
            th: &self.cfg.thresholds,
            findings: &self.findings,
            has_key: self.has_key(),
            llm_advice: self.llm_advice.as_deref(),
            error: self.error.as_deref(),
        }))
    }

    fn refresh_procs_menu(&self) {
        let th = &self.cfg.thresholds;
        let verdicts: Vec<Verdict> = self
            .top
            .by_cpu
            .iter()
            .map(|p| report::judge(p, &self.explanations, th, &self.findings).verdict)
            .collect();
        self.menu.update_procs(&self.top, &verdicts);
    }

    /// Ligne de résumé du menu (« Bilan : 5 normaux, 1 à surveiller »).
    fn refresh_advice_line(&self) {
        if let Some(e) = &self.error {
            self.menu
                .set_advice_line(&format!("Erreur IA : {}", one_line(e, 60)));
        } else if let Some(r) = self.build_report() {
            self.menu.set_advice_line(&r.summary);
        }
    }

    /// Retourne `true` pour quitter.
    fn handle(&mut self, ev: UserEvent) -> bool {
        match ev {
            UserEvent::Snapshot(s) => self.on_snapshot(*s),
            UserEvent::Menu(id) => return self.on_menu(&id),
            UserEvent::Analysis { result, manual } => {
                self.busy = false;
                self.menu.set_busy(false);
                match result {
                    Ok(reply) => {
                        self.explanations.insert_many(reply.items);
                        self.llm_advice = reply.advice;
                        self.error = None;
                        self.finish_analysis(manual);
                    }
                    Err(e) => {
                        dialogs::notify(&e);
                        self.error = Some(e);
                        self.refresh_procs_menu();
                        self.refresh_advice_line();
                        if manual && let Some(r) = self.build_report() {
                            show_dialog(r.text);
                        }
                    }
                }
            }
            UserEvent::Security { procs, startup } => {
                if let Some(p) = procs {
                    self.scan_running = false;
                    self.proc_findings = p;
                }
                if let Some(st) = startup {
                    self.startup_findings = st;
                }
                self.merge_findings();
            }
            UserEvent::ProcAnswer {
                name,
                result,
                security,
            } => {
                self.pending_procs.remove(&name);
                match result {
                    Ok((verdict, text)) if security => {
                        self.ai_opinions
                            .insert(known::normalize(&name), (verdict, text.clone()));
                        self.merge_findings();
                        let local = self
                            .findings
                            .iter()
                            .find(|f| {
                                f.startup.is_none()
                                    && known::normalize(&f.name) == known::normalize(&name)
                            })
                            .map(|f| security_dialog(f, Some(&text)));
                        if let Some(d) = local {
                            show_dialog(d);
                        } else {
                            show_dialog(format!("{} {name}\n\n{text}", verdict.emoji()));
                        }
                    }
                    Ok((verdict, text)) => {
                        self.explanations
                            .insert_many([(name.clone(), verdict, text.clone())]);
                        show_dialog(format!(
                            "{} {} — {}\n\n{text}",
                            verdict.emoji(),
                            known::strip_decor(&name),
                            verdict.label()
                        ));
                        self.refresh_procs_menu();
                        self.refresh_advice_line();
                    }
                    Err(e) => dialogs::notify(&e),
                }
            }
            UserEvent::KeyStatus(p, ok) => {
                self.keys.insert(p, ok);
                self.refresh_advice_line();
            }
            UserEvent::KeyEntered(p, res) => match res {
                Ok(true) => {
                    self.keys.insert(p, true);
                    dialogs::notify(&format!("Clé {} enregistrée dans le Trousseau", p.label()));
                    self.refresh_advice_line();
                }
                Ok(false) => {}
                Err(e) => dialogs::notify(&e),
            },
            UserEvent::ModelChosen(p, model) => {
                let model = model.trim().to_string();
                if !model.is_empty() {
                    self.cfg.models.insert(p.id().to_string(), model.clone());
                    self.cfg.save();
                    dialogs::notify(&format!("Modèle {} : {model}", p.label()));
                }
            }
        }
        false
    }

    fn on_snapshot(&mut self, snap: Snapshot) {
        if let Some(top) = &snap.procs {
            self.top = top.clone();
        }
        self.menu.update_metrics(&snap);

        let sample = Sample {
            cpu: snap.cpu,
            mem_pct: snap.mem_pct(),
            disk_free_pct: snap.disk_free_pct(),
            disk_free_bytes: snap.disk_avail,
            temp: snap.temp,
            top_cpu: self.top.by_cpu.first().map(|p| (p.name.clone(), p.cpu)),
            top_mem: self.top.by_mem.first().map(|p| p.name.clone()),
            procs_fresh: snap.procs.is_some(),
        };
        let update = self.detector.update(&sample);
        self.active = update.active;

        let procs_fresh = snap.procs.is_some();
        self.last = Some(snap);
        self.update_title();
        if procs_fresh {
            self.spawn_scan();
        }
        if self
            .last_startup
            .is_none_or(|t| t.elapsed() >= Duration::from_secs(30 * 60))
        {
            self.last_startup = Some(Instant::now());
            self.spawn_startup_audit();
        }
        if procs_fresh || self.title_first {
            self.title_first = false;
            self.refresh_procs_menu();
            self.refresh_advice_line();
        }

        if !update.started.is_empty() {
            self.on_anomalies(&update.started);
        }
    }

    fn update_title(&mut self) {
        let Some(snap) = &self.last else { return };
        let prefix = if self.findings.iter().any(|f| f.verdict == Verdict::Abnormal) {
            "🛑 "
        } else if self.active.is_empty() {
            ""
        } else {
            "⚠️ "
        };
        let title = format!(
            "{prefix}{:.0}% · {:.0}% · {:.0}G",
            snap.cpu,
            snap.mem_pct(),
            snap.disk_avail as f64 / 1e9
        );
        if title != self.title {
            self.tray.set_title(Some(&title));
            self.title = title;
        }
    }

    fn spawn_scan(&mut self) {
        if self.scan_running {
            return;
        }
        self.scan_running = true;
        let (auditor, top, proxy) = (self.auditor.clone(), self.top.clone(), self.proxy.clone());
        std::thread::spawn(move || {
            let procs = match auditor.try_lock() {
                Ok(mut a) => a.scan_processes(&top).findings,
                Err(_) => Vec::new(),
            };
            let _ = proxy.send_event(UserEvent::Security {
                procs: Some(procs),
                startup: None,
            });
        });
    }

    fn spawn_startup_audit(&self) {
        let (auditor, proxy) = (self.auditor.clone(), self.proxy.clone());
        std::thread::spawn(move || {
            let startup = auditor
                .lock()
                .map(|mut a| a.audit_startup().findings)
                .unwrap_or_default();
            let _ = proxy.send_event(UserEvent::Security {
                procs: None,
                startup: Some(startup),
            });
        });
    }

    fn merge_findings(&mut self) {
        let mut all: Vec<Finding> = self
            .proc_findings
            .iter()
            .chain(&self.startup_findings)
            .cloned()
            .collect();
        // Avis de l'IA : peut aggraver, jamais abaisser un verdict local.
        for f in all.iter_mut().filter(|f| f.startup.is_none()) {
            if let Some((v, t)) = self.ai_opinions.get(&known::normalize(&f.name)) {
                f.apply_ai(*v, t);
            }
        }
        all.sort_by_key(|f| f.verdict != Verdict::Abnormal);
        self.findings = all;
        for f in self
            .findings
            .iter()
            .filter(|f| f.verdict == Verdict::Abnormal)
        {
            let key = f.key();
            if !self.cfg.notified_suspects.contains(&key) {
                let home = security::home();
                let what = match &f.startup {
                    Some(_) => format!("Démarrage automatique suspect : {}", f.name),
                    None => format!("🛑 Logiciel suspect : {}", f.name),
                };
                dialogs::notify(&format!(
                    "{what} — {}",
                    one_line(
                        &format!(
                            "{} {}",
                            f.text(),
                            f.path
                                .as_deref()
                                .map(|p| security::sanitize_path(p, &home))
                                .unwrap_or_default()
                        ),
                        140
                    )
                ));
                self.cfg.notified_suspects.push(key);
                let n = self.cfg.notified_suspects.len();
                if n > 200 {
                    self.cfg.notified_suspects.drain(..n - 200);
                }
                self.cfg.save();
            }
        }
        self.menu.update_security(&self.findings);
        self.update_title();
        self.refresh_procs_menu();
        self.refresh_advice_line();
    }

    fn on_anomalies(&mut self, started: &[Anomaly]) {
        let hint = if self.has_key() {
            ""
        } else {
            " Configure une clé API pour des conseils IA."
        };
        for a in started {
            let sig = a.signature();
            let recent = self
                .notified
                .get(&sig)
                .is_some_and(|t| t.elapsed() < NOTIFY_DEDUPE);
            if !recent {
                self.notified.insert(sig, Instant::now());
                dialogs::notify(&format!("⚠️ {}.{hint}", a.detail));
            }
        }
        let needs_llm = !report::unknown(&self.top, &self.explanations, &self.findings).is_empty();
        if needs_llm && self.has_key() && !self.busy {
            let sigs: Vec<String> = started.iter().map(Anomaly::signature).collect();
            if self.cfg.gate.try_auto(now_secs(), &sigs) {
                self.cfg.save();
                self.start_analysis(false);
            }
        }
    }

    /// Bilan : 100 % local si tous les processus sont connus (ou sans clé) ;
    /// sinon le LLM ne juge que les inconnus.
    fn start_analysis(&mut self, manual: bool) {
        if self.busy {
            return;
        }
        let Some(snap) = self.last.clone() else {
            return;
        };
        let unknown = report::unknown(&self.top, &self.explanations, &self.findings);
        if unknown.is_empty() || !self.has_key() {
            self.llm_advice = None;
            self.finish_analysis(manual);
            return;
        }
        let alerts: Vec<&str> = self.active.iter().map(|a| a.kind.label()).collect();
        let user = prompt::analysis(&snap, &alerts, &unknown);
        let sent: Vec<String> = unknown.iter().map(|p| p.name.clone()).collect();
        let max_tokens = prompt::analysis_tokens(unknown.len());
        let (provider, model) = (self.cfg.provider, self.cfg.model(self.cfg.provider));
        self.busy = true;
        self.menu.set_busy(true);
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let result = call_llm(provider, &model, prompt::SYSTEM, &user, max_tokens)
                .and_then(|t| prompt::parse_reply(&t, &sent).map_err(String::from));
            let _ = proxy.send_event(UserEvent::Analysis { result, manual });
        });
    }

    fn finish_analysis(&mut self, manual: bool) {
        self.refresh_procs_menu();
        self.refresh_advice_line();
        if let Some(r) = self.build_report() {
            dialogs::notify(&r.notify);
            if manual {
                show_dialog(r.text);
            }
        }
    }

    fn on_menu(&mut self, id: &MenuId) -> bool {
        let Some(action) = self.menu.classify(id) else {
            return false;
        };
        match action {
            Action::Quit => return true,
            Action::Analyze => self.start_analysis(true),
            Action::FullAdvice => {
                if let Some(r) = self.build_report() {
                    show_dialog(r.text);
                }
            }
            Action::Process(i) => self.ask_process(i),
            Action::SecReveal(i) => {
                if let Some(p) = self
                    .findings
                    .get(i)
                    .and_then(|f| f.startup.clone().or_else(|| f.path.clone()))
                {
                    reveal_in_finder(p);
                }
            }
            Action::SecKill(i) => {
                if let Some(f) = self.findings.get(i).cloned() {
                    std::thread::spawn(move || force_quit(f));
                }
            }
            Action::ShowPrompt => show_dialog(self.outgoing_prompt()),
            Action::Provider(p) => {
                self.cfg.provider = p;
                self.menu.set_provider(p);
                self.cfg.save();
                self.refresh_advice_line();
            }
            Action::SetKey => {
                let p = self.cfg.provider;
                let proxy = self.proxy.clone();
                std::thread::spawn(move || {
                    let res = match dialogs::ask_text(
                        &format!("Colle ta clé API {}", p.label()),
                        "",
                        true,
                    ) {
                        dialogs::DialogResult::Text(k) if !k.trim().is_empty() => {
                            secrets::set(p, &k).map(|_| true)
                        }
                        dialogs::DialogResult::Failed(e) => Err(e),
                        _ => Ok(false),
                    };
                    let _ = proxy.send_event(UserEvent::KeyEntered(p, res));
                });
            }
            Action::SetModel => {
                let p = self.cfg.provider;
                let current = self.cfg.model(p);
                let proxy = self.proxy.clone();
                std::thread::spawn(move || {
                    if let dialogs::DialogResult::Text(m) =
                        dialogs::ask_text(&format!("Modèle {} :", p.label()), &current, false)
                    {
                        let _ = proxy.send_event(UserEvent::ModelChosen(p, m));
                    }
                });
            }
            Action::Autostart => {
                let want = self.menu.autostart.is_checked();
                match autostart::set_enabled(want) {
                    Ok(()) => {
                        self.cfg.launch_at_login = want;
                        self.cfg.save();
                    }
                    Err(e) => {
                        self.menu.autostart.set_checked(!want);
                        dialogs::notify(&format!("Lancement au démarrage impossible : {e}"));
                    }
                }
            }
        }
        false
    }

    /// Texte exact qui serait envoyé à l'IA pour la prochaine analyse.
    fn outgoing_prompt(&self) -> String {
        let unknown = report::unknown(&self.top, &self.explanations, &self.findings);
        let Some(snap) = &self.last else {
            return "Rien : aucune mesure disponible pour l'instant.".into();
        };
        if unknown.is_empty() {
            return "Rien : tous les processus sont connus, aucune donnée n'est envoyée.".into();
        }
        let alerts: Vec<&str> = self.active.iter().map(|a| a.kind.label()).collect();
        format!(
            "Message système :\n{}\n\nMessage envoyé :\n{}",
            prompt::SYSTEM,
            prompt::analysis(snap, &alerts, &unknown)
        )
    }

    fn ask_process(&mut self, i: usize) {
        let Some(info) = self.top.by_cpu.get(i).cloned() else {
            return;
        };
        let name = known::strip_decor(&info.name).to_string();
        let finding = self
            .findings
            .iter()
            .find(|f| f.startup.is_none() && known::normalize(&f.name) == known::normalize(&name))
            .cloned();
        if let Some(f) = &finding
            && (f.miner || !self.has_key())
        {
            show_dialog(security_dialog(f, None));
            return;
        }
        let k = report::judge(
            &info,
            &self.explanations,
            &self.cfg.thresholds,
            &self.findings,
        );
        if k.verdict != Verdict::Unknown {
            show_dialog(format!(
                "{} {name} — {}\n\n{}",
                k.verdict.emoji(),
                k.verdict.label(),
                k.text
            ));
            return;
        }
        if !self.has_key() {
            show_dialog(format!(
                "❓ {name} — Inconnu\n\nConfigure une clé IA pour identifier ce processus (Fournisseur IA > Configurer la clé API…)."
            ));
            return;
        }
        if !self.pending_procs.insert(info.name.clone()) {
            return;
        }
        let user = match &finding {
            Some(f) => prompt::process_security(
                &info,
                &f.reasons,
                f.path
                    .as_deref()
                    .map(|p| security::sanitize_path(p, &security::home()))
                    .as_deref(),
                f.signature,
            ),
            None => prompt::process(&info),
        };
        let is_security = finding.is_some();
        let sent = vec![info.name.clone()];
        let (provider, model) = (self.cfg.provider, self.cfg.model(self.cfg.provider));
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let result = call_llm(
                provider,
                &model,
                prompt::SYSTEM_PROCESS,
                &user,
                prompt::MAX_TOKENS_PROCESS,
            )
            .and_then(|t| prompt::parse_reply(&t, &sent).map_err(String::from))
            .and_then(|r| {
                r.items
                    .into_iter()
                    .next()
                    .map(|(_, v, t)| (v, t))
                    .ok_or_else(|| prompt::UNREADABLE.to_string())
            });
            let _ = proxy.send_event(UserEvent::ProcAnswer {
                name: info.name,
                result,
                security: is_security,
            });
        });
    }
}

/// Texte du dialogue pour un élément de sécurité (avis IA optionnel).
fn security_dialog(f: &Finding, ai: Option<&str>) -> String {
    let home = security::home();
    let label = if f.verdict == Verdict::Abnormal {
        "Suspect"
    } else {
        "À surveiller"
    };
    let mut t = format!("{} {} — {label}\n\n{}", f.verdict.emoji(), f.name, f.text());
    if let Some(p) = &f.path {
        t.push_str(&format!(
            "\n\nProgramme : {}",
            security::sanitize_path(p, &home)
        ));
    }
    if let Some(a) = ai {
        t.push_str(&format!("\n\nAvis de l'IA : {a}"));
    }
    t
}

fn reveal_in_finder(path: String) {
    std::thread::spawn(move || {
        let _ = std::process::Command::new("/usr/bin/open")
            .args(["-R", &path])
            .status();
    });
}

fn alive(pid: i32) -> bool {
    // SAFETY: signal 0 = simple test d'existence.
    unsafe { libc::kill(pid, 0) == 0 }
}

/// Confirmation puis arrêt : TERM, puis KILL au bout de 3 s si besoin.
fn force_quit(f: Finding) {
    let Some(pid) = f.pid else { return };
    let protected = pid <= 1
        || pid == std::process::id()
        || f.path.as_deref().is_none_or(security::is_system_path);
    if protected {
        dialogs::notify("Refusé : ce processus système ou illisible est protégé.");
        return;
    }
    let msg = format!(
        "Forcer « {} » (PID {pid}) à quitter ?\n\n{}",
        f.name,
        f.text()
    );
    if !dialogs::confirm(&msg, "Forcer à quitter") {
        return;
    }
    if security::pid_path(pid) != f.path {
        dialogs::notify("Le processus a changé entre-temps : abandon.");
        return;
    }
    let p = pid as i32;
    // SAFETY: envoi de signaux à un PID vérifié ci-dessus.
    unsafe { libc::kill(p, libc::SIGTERM) };
    for _ in 0..6 {
        std::thread::sleep(Duration::from_millis(500));
        if !alive(p) {
            dialogs::notify(&format!("{} a été arrêté.", f.name));
            return;
        }
    }
    // SAFETY: idem.
    unsafe { libc::kill(p, libc::SIGKILL) };
    std::thread::sleep(Duration::from_millis(500));
    if alive(p) {
        dialogs::notify(&format!(
            "Impossible d'arrêter {} (droits insuffisants ?).",
            f.name
        ));
    } else {
        dialogs::notify(&format!("{} a été arrêté de force.", f.name));
    }
}

fn show_dialog(text: String) {
    std::thread::spawn(move || dialogs::show_text(&text));
}

fn call_llm(
    provider: Provider,
    model: &str,
    system: &str,
    user: &str,
    max_tokens: u32,
) -> Result<String, String> {
    let key = secrets::get(provider).ok_or_else(|| LlmError::NoKey.to_string())?;
    llm::ask(provider, &key, model, system, user, max_tokens).map_err(|e| {
        eprintln!("llm: {e:?}");
        e.to_string()
    })
}

// ---------------------------------------------------------------------------
// Instance unique et mode --dump
// ---------------------------------------------------------------------------

/// Verrou `flock` : retourne le fichier à garder ouvert, ou `None` si une
/// autre instance tourne déjà.
fn acquire_lock() -> Result<Option<std::fs::File>, ()> {
    use std::os::fd::AsRawFd;
    let Some(path) = config::lock_path() else {
        return Ok(None);
    };
    let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
    else {
        return Ok(None);
    };
    // SAFETY: descripteur valide pour la durée de l'appel.
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc == 0 { Ok(Some(file)) } else { Err(()) }
}

fn dump() {
    let mut s = match Sampler::new() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("sysinfo indisponible : {e}");
            return;
        }
    };
    std::thread::sleep(Duration::from_millis(800));
    let snap = s.sample(Refresh::ALL);
    println!("CPU          : {:.1} %", snap.cpu);
    println!(
        "Mémoire      : {} / {} ({:.1} %)",
        fmt_go(snap.mem_used),
        fmt_go(snap.mem_total),
        snap.mem_pct()
    );
    println!(
        "Disque       : {} libres / {} ({:.1} %)",
        fmt_go(snap.disk_avail),
        fmt_go(snap.disk_total),
        snap.disk_free_pct()
    );
    match snap.temp {
        Some(t) => println!("Température  : {t:.1} °C"),
        None => println!("Température  : n/d"),
    }
    let comps = s.raw_components();
    println!("Capteurs bruts ({}) :", comps.len());
    for (label, t) in comps {
        println!("  {label} = {t:?}");
    }
    if let Some(top) = snap.procs {
        println!("Top CPU :");
        for p in &top.by_cpu {
            println!(
                "  {} ×{} — {:.1} % — {}",
                p.name,
                p.count,
                p.cpu,
                fmt_go(p.mem)
            );
        }
        println!("Top mémoire :");
        for p in &top.by_mem {
            println!(
                "  {} ×{} — {:.1} % — {}",
                p.name,
                p.count,
                p.cpu,
                fmt_go(p.mem)
            );
        }
        let cfg = Config::load();
        let cache = Explanations::load();
        let snap2 = s.sample(Refresh::NONE);
        let unknown = report::unknown(&top, &cache, &[]);
        println!(
            "\nProcessus à soumettre au LLM : {} (0 = aucun appel nécessaire)",
            unknown.len()
        );
        println!(
            "Prompt d'exemple :\n{}",
            prompt::analysis(&snap2, &[], &unknown)
        );
        println!("(fournisseur configuré : {})", cfg.provider.label());
        let r = report::build(&report::Inputs {
            snap: &snap2,
            top: &top,
            cache: &cache,
            th: &cfg.thresholds,
            findings: &[],
            has_key: false,
            llm_advice: None,
            error: None,
        });
        println!(
            "\n===== Bilan local =====\n{}\n\n{} {:?}",
            r.text, r.summary, r.counts
        );
    }
}

/// `--security` : audit complet unique (processus + démarrage automatique).
fn security_cli() {
    let mut sampler = match Sampler::new() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("sysinfo indisponible : {e}");
            return;
        }
    };
    let mut auditor = Auditor::new();
    let mut out = None;
    // Trois relevés rapprochés pour que le suivi « CPU soutenu » puisse jouer.
    for _ in 0..3 {
        std::thread::sleep(Duration::from_millis(1200));
        if let Some(top) = sampler.sample(Refresh::ALL).procs {
            out = Some(auditor.scan_processes(&top));
        }
    }
    println!("== Processus inspectés ==");
    if let Some(o) = &out {
        for l in &o.inspected {
            println!("  {l}");
        }
    }
    let startup = auditor.audit_startup();
    println!(
        "== Démarrage automatique ({} éléments lus) ==",
        startup.inspected.len()
    );
    let all: Vec<&Finding> = out
        .iter()
        .flat_map(|o| &o.findings)
        .chain(&startup.findings)
        .collect();
    println!("== Résultat ==");
    if all.is_empty() {
        println!("✅ Aucun logiciel suspect détecté");
    }
    for f in all {
        println!(
            "{} {} — {} [{}]",
            f.verdict.emoji(),
            f.name,
            f.text(),
            f.path.as_deref().unwrap_or("?")
        );
    }
}

fn main() {
    if std::env::args().any(|a| a == "--security") {
        security_cli();
        return;
    }
    if std::env::args().any(|a| a == "--dump") {
        dump();
        return;
    }
    if let Some(i) = std::env::args().position(|a| a == "--bench") {
        let secs = std::env::args()
            .nth(i + 1)
            .and_then(|v| v.parse().ok())
            .unwrap_or(60);
        bench(secs);
        return;
    }
    let _lock = match acquire_lock() {
        Ok(l) => l,
        Err(()) => {
            eprintln!("Ressource Utility est déjà lancé.");
            return;
        }
    };

    let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    event_loop.set_activation_policy(ActivationPolicy::Accessory);
    let proxy = event_loop.create_proxy();
    let menu_proxy = proxy.clone();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let _ = menu_proxy.send_event(UserEvent::Menu(e.id));
    }));

    let mut app: Option<App> = None;
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::NewEvents(StartCause::Init) => {
                app = App::new(proxy.clone());
                if app.is_some() {
                    spawn_sampler(proxy.clone());
                } else {
                    eprintln!("Impossible de créer l'icône de barre de menus.");
                    *control_flow = ControlFlow::Exit;
                }
            }
            Event::UserEvent(ev) => {
                if let Some(app) = app.as_mut()
                    && app.handle(ev)
                {
                    *control_flow = ControlFlow::Exit;
                }
            }
            _ => {}
        }
    });
}
