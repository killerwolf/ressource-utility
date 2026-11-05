//! Fenêtre « Réglages » native (AppKit via objc2) : créée à la demande,
//! libérée à la fermeture. Les actions sont renvoyées à la boucle d'événements.

use crate::llm::Provider;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSButton, NSColor, NSComboBox, NSControlStateValueOff,
    NSControlStateValueOn, NSEvent, NSFont, NSPopUpButton, NSScreen, NSSecureTextField,
    NSTextField, NSView, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
    NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsAction {
    ProviderChanged,
    Test,
    Save,
    Cancel,
    DeleteKey,
    Closed,
}

pub struct Ivars {
    send: Box<dyn Fn(SettingsAction)>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "RUSettingsTarget"]
    #[ivars = Ivars]
    struct Target;

    unsafe impl NSObjectProtocol for Target {}

    unsafe impl NSWindowDelegate for Target {
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _n: &NSNotification) {
            (self.ivars().send)(SettingsAction::Closed);
        }
    }

    impl Target {
        #[unsafe(method(providerChanged:))]
        fn provider_changed(&self, _s: Option<&AnyObject>) {
            (self.ivars().send)(SettingsAction::ProviderChanged);
        }
        #[unsafe(method(testClicked:))]
        fn test_clicked(&self, _s: Option<&AnyObject>) {
            (self.ivars().send)(SettingsAction::Test);
        }
        #[unsafe(method(saveClicked:))]
        fn save_clicked(&self, _s: Option<&AnyObject>) {
            (self.ivars().send)(SettingsAction::Save);
        }
        #[unsafe(method(cancelClicked:))]
        fn cancel_clicked(&self, _s: Option<&AnyObject>) {
            (self.ivars().send)(SettingsAction::Cancel);
        }
        #[unsafe(method(deleteKeyClicked:))]
        fn delete_key_clicked(&self, _s: Option<&AnyObject>) {
            (self.ivars().send)(SettingsAction::DeleteKey);
        }
    }
);

impl Target {
    fn new(mtm: MainThreadMarker, send: Box<dyn Fn(SettingsAction)>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars { send });
        // SAFETY: `init` de NSObject.
        unsafe { msg_send![super(this), init] }
    }
}

/// Valeurs lues dans le formulaire.
pub struct Form {
    pub provider: Provider,
    pub model: String,
    pub key: String,
    pub base_url: String,
    pub autostart: bool,
}

/// État à afficher pour un fournisseur.
pub struct View<'a> {
    pub provider: Provider,
    pub model: &'a str,
    pub suggestions: &'a [String],
    pub key_exists: bool,
    pub base_url: &'a str,
}

pub struct SettingsWindow {
    window: Retained<NSWindow>,
    _target: Retained<Target>,
    popup: Retained<NSPopUpButton>,
    combo: Retained<NSComboBox>,
    key_label: Retained<NSTextField>,
    key_field: Retained<NSSecureTextField>,
    key_delete: Retained<NSButton>,
    url_label: Retained<NSTextField>,
    url_field: Retained<NSTextField>,
    hint: Retained<NSTextField>,
    autostart: Retained<NSButton>,
    status: Retained<NSTextField>,
}

fn s(text: &str) -> Retained<NSString> {
    NSString::from_str(text)
}

fn rect(x: f64, top: f64, w: f64, h: f64) -> NSRect {
    // Origine AppKit en bas à gauche ; la fenêtre fait 330 pt de haut.
    NSRect::new(NSPoint::new(x, 330.0 - top - h), NSSize::new(w, h))
}

fn add(parent: &NSView, child: &NSView) {
    parent.addSubview(child);
}

fn label(mtm: MainThreadMarker, text: &str, f: NSRect) -> Retained<NSTextField> {
    let l = NSTextField::labelWithString(&s(text), mtm);
    l.setFrame(f);
    l
}

impl SettingsWindow {
    pub fn open(
        mtm: MainThreadMarker,
        send: Box<dyn Fn(SettingsAction)>,
        autostart: bool,
    ) -> Option<Self> {
        let target = Target::new(mtm, send);
        let window = {
            let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable;
            // SAFETY: création standard d'une fenêtre sur le thread principal.
            unsafe {
                NSWindow::initWithContentRect_styleMask_backing_defer(
                    mtm.alloc(),
                    NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(440.0, 330.0)),
                    style,
                    NSBackingStoreType::Buffered,
                    false,
                )
            }
        };
        // SAFETY: la fenêtre est conservée par `SettingsWindow`, pas par AppKit.
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&s("Réglages — Ressource Utility"));
        window.setDelegate(Some(ProtocolObject::from_ref(&*target)));
        let content = window.contentView()?;

        add(
            &content,
            &label(mtm, "Fournisseur", rect(20.0, 22.0, 115.0, 20.0)),
        );
        let popup = NSPopUpButton::initWithFrame_pullsDown(
            mtm.alloc(),
            rect(140.0, 18.0, 280.0, 26.0),
            false,
        );
        for p in Provider::ALL {
            popup.addItemWithTitle(&s(p.label()));
        }
        // SAFETY: cible et action valides (méthode définie sur `Target`).
        unsafe {
            popup.setTarget(Some(&target));
            popup.setAction(Some(sel!(providerChanged:)));
        }
        add(&content, &popup);

        add(
            &content,
            &label(mtm, "Modèle", rect(20.0, 64.0, 115.0, 20.0)),
        );
        let combo = NSComboBox::initWithFrame(mtm.alloc(), rect(140.0, 60.0, 280.0, 26.0));
        combo.setCompletes(true);
        combo.setNumberOfVisibleItems(12);
        add(&content, &combo);

        let key_label = label(mtm, "Clé API", rect(20.0, 106.0, 115.0, 20.0));
        add(&content, &key_label);
        let key_field =
            NSSecureTextField::initWithFrame(mtm.alloc(), rect(140.0, 102.0, 280.0, 26.0));
        add(&content, &key_field);
        // SAFETY: cible et sélecteur définis sur `Target`.
        let key_delete = unsafe {
            NSButton::buttonWithTitle_target_action(
                &s("Supprimer la clé"),
                Some(&target),
                Some(sel!(deleteKeyClicked:)),
                mtm,
            )
        };
        key_delete.setFrame(rect(300.0, 130.0, 125.0, 26.0));
        add(&content, &key_delete);

        let url_label = label(mtm, "Adresse du serveur", rect(20.0, 106.0, 120.0, 20.0));
        add(&content, &url_label);
        let url_field = NSTextField::initWithFrame(mtm.alloc(), rect(140.0, 102.0, 280.0, 26.0));
        add(&content, &url_field);

        let hint = label(mtm, "", rect(140.0, 134.0, 280.0, 18.0));
        hint.setFont(Some(&NSFont::systemFontOfSize(11.0)));
        hint.setTextColor(Some(&NSColor::secondaryLabelColor()));
        add(&content, &hint);

        // SAFETY: pas de cible ni d'action.
        let auto = unsafe {
            NSButton::checkboxWithTitle_target_action(&s("Lancer au démarrage"), None, None, mtm)
        };
        auto.setFrame(rect(140.0, 164.0, 280.0, 20.0));
        auto.setState(if autostart {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
        add(&content, &auto);

        let status = NSTextField::wrappingLabelWithString(&s(""), mtm);
        status.setFrame(rect(20.0, 200.0, 400.0, 70.0));
        add(&content, &status);

        let mk = |title: &str, sel, x: f64| {
            // SAFETY: cible et sélecteurs définis sur `Target`.
            let b = unsafe {
                NSButton::buttonWithTitle_target_action(&s(title), Some(&target), Some(sel), mtm)
            };
            b.setFrame(rect(x, 286.0, 95.0, 32.0));
            b
        };
        let test = mk("Tester", sel!(testClicked:), 20.0);
        let cancel = mk("Annuler", sel!(cancelClicked:), 230.0);
        cancel.setKeyEquivalent(&s("\u{1b}"));
        let save = mk("Enregistrer", sel!(saveClicked:), 330.0);
        save.setKeyEquivalent(&s("\r"));
        add(&content, &test);
        add(&content, &cancel);
        add(&content, &save);

        // Centre la fenêtre sur l'écran où se trouve le pointeur (là où on a cliqué).
        let mouse = NSEvent::mouseLocation();
        let screens = NSScreen::screens(mtm);
        let hit = screens.iter().find(|sc| {
            let f = sc.frame();
            mouse.x >= f.origin.x
                && mouse.x <= f.origin.x + f.size.width
                && mouse.y >= f.origin.y
                && mouse.y <= f.origin.y + f.size.height
        });
        match hit.or_else(|| screens.iter().next()) {
            Some(sc) => {
                let f = sc.visibleFrame();
                let w = window.frame().size;
                window.setFrameOrigin(NSPoint::new(
                    f.origin.x + (f.size.width - w.width) / 2.0,
                    f.origin.y + (f.size.height - w.height) / 2.0,
                ));
            }
            None => window.center(),
        }
        window.makeKeyAndOrderFront(None);
        window.orderFrontRegardless();
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);

        Some(Self {
            window,
            _target: target,
            popup,
            combo,
            key_label,
            key_field,
            key_delete,
            url_label,
            url_field,
            hint,
            autostart: auto,
            status,
        })
    }

    /// Écrit le rendu du contenu de la fenêtre en PNG (test visuel `--settings-demo`,
    /// sans capture d'écran).
    pub fn snapshot_png(&self, path: &str) -> bool {
        use objc2_app_kit::{NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua};
        use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep};
        use objc2_foundation::NSDictionary;
        // SAFETY: constante AppKit.
        let aqua = unsafe { NSAppearance::appearanceNamed(NSAppearanceNameAqua) };
        self.window.setAppearance(aqua.as_deref());
        self.window.displayIfNeeded();
        let Some(view) = self.window.contentView() else {
            return false;
        };
        let bounds = view.bounds();
        let Some(rep) = view.bitmapImageRepForCachingDisplayInRect(bounds) else {
            return false;
        };
        view.cacheDisplayInRect_toBitmapImageRep(bounds, &rep);
        let rep: &NSBitmapImageRep = &rep;
        // SAFETY: dictionnaire vide valide.
        let data = unsafe {
            rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
        };
        data.is_some_and(|d| d.writeToFile_atomically(&s(path), true))
    }

    pub fn close(&self) {
        self.window.setDelegate(None);
        self.window.close();
    }

    pub fn form(&self) -> Form {
        let idx = self.popup.indexOfSelectedItem().max(0) as usize;
        Form {
            provider: Provider::ALL
                .get(idx)
                .copied()
                .unwrap_or(Provider::Anthropic),
            model: self.combo.stringValue().to_string().trim().to_string(),
            key: self.key_field.stringValue().to_string().trim().to_string(),
            base_url: self.url_field.stringValue().to_string().trim().to_string(),
            autostart: self.autostart.state() == NSControlStateValueOn,
        }
    }

    /// Affiche les champs adaptés au fournisseur.
    pub fn show(&self, v: &View) {
        let idx = Provider::ALL
            .iter()
            .position(|p| *p == v.provider)
            .unwrap_or(0);
        self.popup.selectItemAtIndex(idx as isize);
        let local = v.provider == Provider::LmStudio;
        self.key_label.setHidden(local);
        self.key_field.setHidden(local);
        self.key_delete.setHidden(local || !v.key_exists);
        self.url_label.setHidden(!local);
        self.url_field.setHidden(!local);
        self.url_field.setStringValue(&s(v.base_url));
        self.key_field.setStringValue(&s(""));
        self.key_field
            .setPlaceholderString(Some(&s(if v.key_exists {
                "•••• enregistrée dans le Trousseau"
            } else {
                "Colle ta clé API ici"
            })));
        self.hint.setStringValue(&s(if local {
            "100 % local, rien ne sort du Mac"
        } else {
            "Dans le Trousseau macOS"
        }));
        self.combo.setStringValue(&s(v.model));
        self.set_models(v.suggestions);
        self.set_status("");
    }

    /// Remplace la liste de suggestions sans toucher au texte saisi.
    pub fn set_models(&self, models: &[String]) {
        self.combo.removeAllItems();
        for m in models {
            // SAFETY: l'objet ajouté est une NSString.
            unsafe { self.combo.addItemWithObjectValue(&s(m)) };
        }
    }

    /// Prérempli le modèle seulement si le champ est vide.
    pub fn fill_model_if_empty(&self, model: &str) {
        if self.combo.stringValue().to_string().trim().is_empty() {
            self.combo.setStringValue(&s(model));
        }
    }

    pub fn set_key_exists(&self, exists: bool) {
        self.key_delete.setHidden(!exists);
        self.key_field.setStringValue(&s(""));
        self.key_field.setPlaceholderString(Some(&s(if exists {
            "•••• enregistrée dans le Trousseau"
        } else {
            "Colle ta clé API ici"
        })));
    }

    pub fn set_status(&self, text: &str) {
        self.status.setStringValue(&s(text));
    }
}
