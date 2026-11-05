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

use anomaly::{Anomaly, Detector, Sample};
use config::{Config, Explanations};
use known::Verdict;
use llm::{LlmError, Provider};
use menu::{Action, AppMenu, one_line};
use metrics::{ProcTop, Refresh, Sampler, Snapshot, fmt_go};
use std::collections::{HashMap, HashSet};
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
    sample_loop(|_| start.elapsed() < Duration::from_secs(secs));
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
            .map(|p| report::judge(p, &self.explanations, th).verdict)
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
            UserEvent::ProcAnswer { name, result } => {
                self.pending_procs.remove(&name);
                match result {
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

        let warn = if self.active.is_empty() {
            ""
        } else {
            "⚠️ "
        };
        let free_g = snap.disk_avail as f64 / 1e9;
        let title = format!(
            "{warn}{:.0}% · {:.0}% · {:.0}G",
            snap.cpu,
            snap.mem_pct(),
            free_g
        );
        if title != self.title {
            self.tray.set_title(Some(&title));
            self.title = title;
        }
        let procs_fresh = snap.procs.is_some();
        self.last = Some(snap);
        if procs_fresh || self.title_first {
            self.title_first = false;
            self.refresh_procs_menu();
            self.refresh_advice_line();
        }

        if !update.started.is_empty() {
            self.on_anomalies(&update.started);
        }
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
        let needs_llm = !report::unknown(&self.top, &self.explanations).is_empty();
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
        let unknown = report::unknown(&self.top, &self.explanations);
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

    fn ask_process(&mut self, i: usize) {
        let Some(info) = self.top.by_cpu.get(i).cloned() else {
            return;
        };
        let name = known::strip_decor(&info.name).to_string();
        let k = report::judge(&info, &self.explanations, &self.cfg.thresholds);
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
        let user = prompt::process(&info);
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
            });
        });
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
        let unknown = report::unknown(&top, &cache);
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

fn main() {
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
