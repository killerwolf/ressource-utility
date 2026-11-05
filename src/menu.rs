//! Menu de la barre de menus : créé une fois, mis à jour sur place.

use crate::known::Verdict;
use crate::llm::Provider;
use crate::metrics::{ProcTop, Snapshot, fmt_go, fmt_mem};
use std::cell::RefCell;
use tray_icon::menu::{CheckMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu};

pub const PROC_SLOTS: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Process(usize),
    FullAdvice,
    Analyze,
    Provider(Provider),
    SetKey,
    SetModel,
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

pub struct AppMenu {
    pub menu: Menu,
    cpu: Line,
    mem: Line,
    disk: Line,
    temp: Line,
    procs: Vec<Line>,
    advice: Line,
    full: MenuItem,
    analyze: MenuItem,
    providers: Vec<(Provider, CheckMenuItem)>,
    set_key: MenuItem,
    set_model: MenuItem,
    pub autostart: CheckMenuItem,
    quit: MenuItem,
}

fn info(text: &str) -> MenuItem {
    MenuItem::new(text, false, None)
}

impl AppMenu {
    pub fn build(provider: Provider, autostart: bool) -> Result<Self, tray_icon::menu::Error> {
        let menu = Menu::new();
        let cpu = Line::new(info("CPU : …"));
        let mem = Line::new(info("Mémoire : …"));
        let disk = Line::new(info("Disque : …"));
        let temp = Line::new(info("Température : …"));

        let procs_sub = Submenu::new("Processus les plus gourmands", true);
        let procs: Vec<Line> = (0..PROC_SLOTS).map(|_| Line::new(info("—"))).collect();
        for p in &procs {
            procs_sub.append(&p.item)?;
        }

        let advice = Line::new(info("Bilan : en attente des mesures"));
        let full = MenuItem::new("Voir le bilan", true, None);
        let analyze = MenuItem::new("Analyser maintenant", true, None);

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
        prov_sub.append(&PredefinedMenuItem::separator())?;
        let set_key = MenuItem::new("Configurer la clé API…", true, None);
        let set_model = MenuItem::new("Changer le modèle…", true, None);
        prov_sub.append(&set_key)?;
        prov_sub.append(&set_model)?;

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
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&prov_sub)?;
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
            providers,
            set_key,
            set_model,
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
        let table = [
            (self.full.id(), Action::FullAdvice),
            (self.analyze.id(), Action::Analyze),
            (self.set_key.id(), Action::SetKey),
            (self.set_model.id(), Action::SetModel),
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
