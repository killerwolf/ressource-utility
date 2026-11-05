//! Menu de la barre de menus : créé une fois, mis à jour sur place.

use crate::known::Verdict;
use crate::llm::Provider;
use crate::metrics::{ProcTop, Snapshot, fmt_go, fmt_mem};
use crate::security::Finding;
use std::cell::RefCell;
use tray_icon::menu::{
    CheckMenuItem, Icon as MenuIcon, IconMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem,
    Submenu,
};

pub const PROC_SLOTS: usize = 5;
const SEC_SLOTS: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Process(usize),
    SecReveal(usize),
    SecKill(usize),
    ShowPrompt,
    FullAdvice,
    Analyze,
    Provider(Provider),
    Settings,
    Autostart,
    Quit,
}

/// Élément de menu dont le texte n'est réécrit que s'il change.
struct Line {
    item: MenuItem,
    last: RefCell<String>,
}

impl Line {
    fn new(item: MenuItem) -> Self {
        Self {
            item,
            last: RefCell::new(String::new()),
        }
    }

    fn set(&self, text: impl Into<String>) {
        let text = text.into();
        if *self.last.borrow() != text {
            self.item.set_text(&text);
            *self.last.borrow_mut() = text;
        }
    }
}

struct SecSlot {
    sub: Submenu,
    reveal: MenuItem,
    kill: MenuItem,
}

/// Ligne d'information avec mini-courbe en icône.
struct ChartLine {
    item: IconMenuItem,
    last: RefCell<String>,
}

impl ChartLine {
    fn new(text: &str) -> Self {
        Self {
            item: IconMenuItem::new(text, false, None, None),
            last: RefCell::new(String::new()),
        }
    }

    fn set(&self, text: String) {
        if *self.last.borrow() != text {
            self.item.set_text(&text);
            *self.last.borrow_mut() = text;
        }
    }

    fn set_chart(&self, values: &[f32], lo: f32, hi: f32) {
        if values.is_empty() {
            self.item.set_icon(None);
        } else if let Ok(icon) = MenuIcon::from_rgba(
            crate::chart::render(values, lo, hi),
            crate::chart::W,
            crate::chart::H,
        ) {
            self.item.set_icon(Some(icon));
        }
    }
}

pub struct AppMenu {
    pub menu: Menu,
    cpu: ChartLine,
    mem: ChartLine,
    disk: Line,
    temp: ChartLine,
    procs: Vec<Line>,
    advice: Line,
    full: MenuItem,
    analyze: MenuItem,
    show_prompt: MenuItem,
    sec_root: Submenu,
    sec_status: Line,
    sec_slots: Vec<SecSlot>,
    providers: Vec<(Provider, CheckMenuItem)>,
    settings: MenuItem,
    pub autostart: CheckMenuItem,
    quit: MenuItem,
}

fn info(text: &str) -> MenuItem {
    MenuItem::new(text, false, None)
}

impl AppMenu {
    pub fn build(provider: Provider, autostart: bool) -> Result<Self, tray_icon::menu::Error> {
        let menu = Menu::new();
        let cpu = ChartLine::new("CPU : …");
        let mem = ChartLine::new("Mémoire : …");
        let disk = Line::new(info("Disque : …"));
        let temp = ChartLine::new("Température : …");

        let procs_sub = Submenu::new("Processus les plus gourmands", true);
        let procs: Vec<Line> = (0..PROC_SLOTS).map(|_| Line::new(info("—"))).collect();
        for p in &procs {
            procs_sub.append(&p.item)?;
        }

        let advice = Line::new(info("Bilan : en attente des mesures"));
        let full = MenuItem::new("Voir le bilan", true, None);
        let analyze = MenuItem::new("Analyser maintenant", true, None);

        let show_prompt = MenuItem::new("Voir ce qui sera envoyé à l'IA", true, None);

        let sec_root = Submenu::new("✅ Sécurité", true);
        let sec_status = Line::new(info("Aucun logiciel suspect détecté"));
        sec_root.append(&sec_status.item)?;
        let mut sec_slots = Vec::new();
        for _ in 0..SEC_SLOTS {
            let sub = Submenu::new("—", false);
            let reveal = MenuItem::new("Afficher dans le Finder", true, None);
            let kill = MenuItem::new("Forcer à quitter…", true, None);
            sub.append(&reveal)?;
            sub.append(&kill)?;
            sec_root.append(&sub)?;
            sec_slots.push(SecSlot { sub, reveal, kill });
        }

        let prov_sub = Submenu::new("Fournisseur IA", true);
        let providers: Vec<(Provider, CheckMenuItem)> = Provider::ALL
            .iter()
            .map(|p| {
                (
                    *p,
                    CheckMenuItem::new(p.label(), true, *p == provider, None),
                )
            })
            .collect();
        for (_, item) in &providers {
            prov_sub.append(item)?;
        }

        let settings = MenuItem::new("Réglages…", true, None);
        let autostart = CheckMenuItem::new("Lancer au démarrage", true, autostart, None);
        let quit = MenuItem::new("Quitter", true, None);

        menu.append(&cpu.item)?;
        menu.append(&mem.item)?;
        menu.append(&disk.item)?;
        menu.append(&temp.item)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&procs_sub)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&advice.item)?;
        menu.append(&full)?;
        menu.append(&analyze)?;
        menu.append(&show_prompt)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&sec_root)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&prov_sub)?;
        menu.append(&settings)?;
        menu.append(&autostart)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&quit)?;

        Ok(Self {
            menu,
            cpu,
            mem,
            disk,
            temp,
            procs,
            advice,
            full,
            analyze,
            show_prompt,
            sec_root,
            sec_status,
            sec_slots,
            providers,
            settings,
            autostart,
            quit,
        })
    }

    pub fn classify(&self, id: &MenuId) -> Option<Action> {
        if let Some(i) = self.procs.iter().position(|p| p.item.id() == id) {
            return Some(Action::Process(i));
        }
        if let Some((p, _)) = self.providers.iter().find(|(_, c)| c.id() == id) {
            return Some(Action::Provider(*p));
        }
        if let Some(i) = self.sec_slots.iter().position(|s| s.reveal.id() == id) {
            return Some(Action::SecReveal(i));
        }
        if let Some(i) = self.sec_slots.iter().position(|s| s.kill.id() == id) {
            return Some(Action::SecKill(i));
        }
        let table = [
            (self.show_prompt.id(), Action::ShowPrompt),
            (self.full.id(), Action::FullAdvice),
            (self.analyze.id(), Action::Analyze),
            (self.settings.id(), Action::Settings),
            (self.autostart.id(), Action::Autostart),
            (self.quit.id(), Action::Quit),
        ];
        table.iter().find(|(i, _)| *i == id).map(|(_, a)| *a)
    }

    pub fn update_metrics(&self, s: &Snapshot) {
        self.cpu.set(format!("CPU : {:.0} %", s.cpu));
        self.mem.set(format!(
            "Mémoire : {} / {} ({:.0} %)",
            fmt_go(s.mem_used).replace(" Go", ""),
            fmt_go(s.mem_total),
            s.mem_pct()
        ));
        self.disk.set(format!(
            "Disque : {} libres ({:.0} %)",
            fmt_go(s.disk_avail),
            s.disk_free_pct()
        ));
        self.temp.set(match s.temp {
            Some(t) => format!("Température : {t:.0} °C"),
            None => "Température : n/d".to_string(),
        });
    }

    pub fn update_procs(&self, top: &ProcTop, verdicts: &[Verdict]) {
        for (i, item) in self.procs.iter().enumerate() {
            match top.by_cpu.get(i) {
                Some(p) => {
                    let name = if p.count > 1 {
                        format!("{} ×{}", p.name, p.count)
                    } else {
                        p.name.clone()
                    };
                    let v = verdicts.get(i).copied().unwrap_or(Verdict::Unknown);
                    item.set(format!(
                        "{} {name} — {:.0} % CPU — {}",
                        v.emoji(),
                        p.cpu,
                        fmt_mem(p.mem)
                    ));
                    item.item.set_enabled(true);
                }
                None => {
                    item.set("—");
                    item.item.set_enabled(false);
                }
            }
        }
    }

    /// Mini-courbes de la dernière heure (appelé une fois par minute).
    pub fn update_charts(&self, h: &crate::history::History) {
        self.cpu.set_chart(&h.cpu.values(), 0.0, 100.0);
        self.mem.set_chart(&h.mem.values(), 0.0, 100.0);
        self.temp.set_chart(&h.temp.values(), 20.0, 100.0);
    }

    pub fn update_security(&self, findings: &[Finding]) {
        use crate::known::Verdict;
        let worst = if findings.iter().any(|f| f.verdict == Verdict::Abnormal) {
            "🛑"
        } else if findings.is_empty() {
            "✅"
        } else {
            "👀"
        };
        self.sec_root.set_text(format!("{worst} Sécurité"));
        self.sec_status.set(if findings.is_empty() {
            "Aucun logiciel suspect détecté".to_string()
        } else {
            format!("{} élément(s) à vérifier", findings.len())
        });
        for (i, slot) in self.sec_slots.iter().enumerate() {
            match findings.get(i) {
                Some(f) => {
                    slot.sub
                        .set_text(format!("{} {}", f.verdict.emoji(), f.name));
                    slot.sub.set_enabled(true);
                    slot.kill.set_enabled(f.pid.is_some());
                }
                None => {
                    slot.sub.set_text("—");
                    slot.sub.set_enabled(false);
                }
            }
        }
    }

    pub fn set_advice_line(&self, text: &str) {
        self.advice.set(text);
    }

    pub fn set_busy(&self, busy: bool) {
        self.analyze.set_enabled(!busy);
        self.analyze.set_text(if busy {
            "Analyse en cours…"
        } else {
            "Analyser maintenant"
        });
    }

    pub fn set_provider(&self, current: Provider) {
        for (p, item) in &self.providers {
            item.set_checked(*p == current);
        }
    }
}

/// Texte sur une ligne, tronqué pour le menu.
pub fn one_line(text: &str, max: usize) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        flat
    } else {
        let cut: String = flat.chars().take(max).collect();
        format!("{}…", cut.trim_end())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_line_truncates() {
        assert_eq!(one_line("a\n  b", 10), "a b");
        assert_eq!(one_line("abcdefghij", 5), "abcde…");
    }
}
