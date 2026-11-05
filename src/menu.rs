//! Menu de la barre de menus : créé une fois, mis à jour sur place.
//!
//! Un seul endroit par notion : état, métriques, processus, bilan, réglages.

use crate::metrics::Snapshot;
use crate::metrics::fmt_go;
use crate::ux::{Entry, SLOTS};
use std::cell::RefCell;
use tray_icon::menu::accelerator::{Accelerator, Code, Modifiers};
use tray_icon::menu::{
    Icon as MenuIcon, IconMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Ligne n° i de la liste « Processus ».
    Process(usize),
    Report,
    Settings,
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
    headline: Line,
    cpu: ChartLine,
    mem: ChartLine,
    disk: Line,
    temp: ChartLine,
    procs: Vec<Line>,
    report: MenuItem,
    settings: MenuItem,
    quit: MenuItem,
}

fn info(text: &str) -> MenuItem {
    MenuItem::new(text, false, None)
}

impl AppMenu {
    /// Structure :
    /// phrase d'état · CPU / Mémoire / Disque / Température · Processus ▸ ·
    /// Bilan de santé… · Réglages… (⌘,) · Quitter (⌘Q).
    pub fn build() -> Result<Self, tray_icon::menu::Error> {
        let menu = Menu::new();
        let headline = Line::new(info("Mesures en cours…"));
        let cpu = ChartLine::new("CPU …");
        let mem = ChartLine::new("Mémoire …");
        let disk = Line::new(info("Disque …"));
        let temp = ChartLine::new("Température …");

        let procs_sub = Submenu::new("Processus", true);
        let procs: Vec<Line> = (0..SLOTS)
            .map(|_| Line::new(MenuItem::new("—", false, None)))
            .collect();
        for p in &procs {
            procs_sub.append(&p.item)?;
        }

        let report = MenuItem::new("Bilan de santé…", true, None);
        let settings = MenuItem::new(
            "Réglages…",
            true,
            Some(Accelerator::new(Modifiers::META, Code::Comma)),
        );
        let quit = MenuItem::new(
            "Quitter",
            true,
            Some(Accelerator::new(Modifiers::META, Code::KeyQ)),
        );

        menu.append(&headline.item)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&cpu.item)?;
        menu.append(&mem.item)?;
        menu.append(&disk.item)?;
        menu.append(&temp.item)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&procs_sub)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&report)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&settings)?;
        menu.append(&quit)?;

        Ok(Self {
            menu,
            headline,
            cpu,
            mem,
            disk,
            temp,
            procs,
            report,
            settings,
            quit,
        })
    }

    pub fn classify(&self, id: &MenuId) -> Option<Action> {
        if let Some(i) = self.procs.iter().position(|p| p.item.id() == id) {
            return Some(Action::Process(i));
        }
        let table = [
            (self.report.id(), Action::Report),
            (self.settings.id(), Action::Settings),
            (self.quit.id(), Action::Quit),
        ];
        table.iter().find(|(i, _)| *i == id).map(|(_, a)| *a)
    }

    pub fn set_headline(&self, text: &str) {
        self.headline.set(text);
    }

    pub fn update_metrics(&self, s: &Snapshot) {
        self.cpu.set(format!("CPU {:.0} %", s.cpu));
        self.mem.set(format!(
            "Mémoire {:.0} % · {} / {}",
            s.mem_pct(),
            fmt_go(s.mem_used).replace(" Go", ""),
            fmt_go(s.mem_total),
        ));
        self.disk.set(format!(
            "Disque {} libres ({:.0} %)",
            fmt_go(s.disk_avail),
            s.disk_free_pct()
        ));
        self.temp.set(match s.temp {
            Some(t) => format!("Température {t:.0} °C"),
            None => "Température n/d".to_string(),
        });
    }

    /// Liste « Processus » : suspects d'abord, puis les plus gourmands.
    pub fn update_procs(&self, entries: &[Entry]) {
        for (i, slot) in self.procs.iter().enumerate() {
            match entries.get(i) {
                Some(e) => {
                    slot.set(e.label.clone());
                    slot.item.set_enabled(true);
                }
                None => {
                    slot.set("—");
                    slot.item.set_enabled(false);
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

    /// « Bilan de santé… » ou « Bilan en cours… » (désactivé) pendant l'analyse.
    pub fn set_busy(&self, busy: bool) {
        self.report.set_enabled(!busy);
        self.report.set_text(if busy {
            "Bilan en cours…"
        } else {
            "Bilan de santé…"
        });
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
