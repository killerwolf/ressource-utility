//! Configuration (non secrète) et cache d'explications, en JSON.

use crate::anomaly::{Gate, Thresholds};
use crate::known::{self, Verdict};
use crate::llm::Provider;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub provider: Provider,
    pub models: HashMap<String, String>,
    pub thresholds: Thresholds,
    pub launch_at_login: bool,
    pub gate: Gate,
    /// Exécutables suspects déjà signalés (une seule notification chacun).
    pub notified_suspects: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: Provider::Anthropic,
            models: HashMap::new(),
            thresholds: Thresholds::default(),
            launch_at_login: false,
            gate: Gate::default(),
            notified_suspects: Vec::new(),
        }
    }
}

impl Config {
    pub fn model(&self, p: Provider) -> String {
        self.models
            .get(p.id())
            .filter(|m| !m.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| p.default_model().to_string())
    }

    pub fn load() -> Self {
        let mut c: Self = load_json("config.json");
        c.migrate();
        c
    }

    /// `openrouter/free` pouvait renvoyer des modèles de raisonnement :
    /// on repasse sur le modèle par défaut actuel.
    fn migrate(&mut self) {
        if self.models.get("openrouter").map(String::as_str) == Some("openrouter/free") {
            self.models.remove("openrouter");
        }
    }

    pub fn save(&self) {
        save_json("config.json", self);
    }
}

/// Verdict mis en cache pour un processus (jamais redemandé au LLM).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CachedVerdict {
    pub verdict: Verdict,
    pub text: String,
}

/// Verdicts de processus mis en cache à vie par nom (sans décoration).
#[derive(Default, Serialize)]
pub struct Explanations(pub HashMap<String, CachedVerdict>);

impl Explanations {
    pub fn load() -> Self {
        let raw: HashMap<String, serde_json::Value> = load_json("explanations.json");
        Self::from_raw(raw)
    }

    /// Les anciennes entrées en texte brut (v0.1) sont abandonnées.
    fn from_raw(raw: HashMap<String, serde_json::Value>) -> Self {
        Self(
            raw.into_iter()
                .filter_map(|(k, v)| Some((k, serde_json::from_value(v).ok()?)))
                .collect(),
        )
    }

    pub fn get(&self, name: &str) -> Option<&CachedVerdict> {
        self.0.get(&known::normalize(name))
    }

    pub fn insert_many(&mut self, items: impl IntoIterator<Item = (String, Verdict, String)>) {
        for (name, verdict, text) in items {
            self.0
                .insert(known::normalize(&name), CachedVerdict { verdict, text });
        }
        save_json("explanations.json", self);
    }
}

fn dir() -> Option<PathBuf> {
    let d = dirs::config_dir()?.join("ressource-utility");
    std::fs::create_dir_all(&d).ok()?;
    Some(d)
}

pub fn lock_path() -> Option<PathBuf> {
    dir().map(|d| d.join("ressource-utility.lock"))
}

fn load_json<T: DeserializeOwned + Default>(file: &str) -> T {
    dir()
        .and_then(|d| std::fs::read(d.join(file)).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_json<T: Serialize>(file: &str, value: &T) {
    let Some(d) = dir() else { return };
    let Ok(bytes) = serde_json::to_vec_pretty(value) else {
        return;
    };
    let tmp = d.join(format!("{file}.tmp"));
    if std::fs::write(&tmp, bytes).is_ok() && std::fs::rename(&tmp, d.join(file)).is_err() {
        eprintln!("config: écriture de {file} impossible");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_falls_back_to_default() {
        let mut c = Config::default();
        assert_eq!(c.model(Provider::Gemini), "gemini-2.5-flash");
        c.models.insert("gemini".into(), "gemini-2.5-pro".into());
        assert_eq!(c.model(Provider::Gemini), "gemini-2.5-pro");
    }

    #[test]
    fn partial_json_uses_defaults() {
        let c: Config = serde_json::from_str(r#"{"provider":"openai"}"#).unwrap();
        assert_eq!(c.provider, Provider::OpenAi);
        assert_eq!(c.thresholds.cpu_samples, 5);
    }

    #[test]
    fn v01_config_without_openrouter_loads() {
        let json = r#"{"provider":"gemini","models":{"gemini":"gemini-2.5-pro"},
            "thresholds":{"cpu_pct":85.0},"launch_at_login":true,
            "gate":{"global_secs":1800,"signature_secs":7200,"last_auto":5,"signatures":{}}}"#;
        let c: Config = serde_json::from_str(json).unwrap();
        assert_eq!(c.provider, Provider::Gemini);
        assert_eq!(c.model(Provider::OpenRouter), "google/gemma-4-31b-it:free");
        assert_eq!(c.model(Provider::Gemini), "gemini-2.5-pro");
        let c2: Config = serde_json::from_str(r#"{"provider":"openrouter"}"#).unwrap();
        assert_eq!(c2.provider, Provider::OpenRouter);
    }

    #[test]
    fn old_openrouter_model_is_migrated() {
        let mut c: Config =
            serde_json::from_str(r#"{"models":{"openrouter":"openrouter/free","gemini":"x"}}"#)
                .unwrap();
        c.migrate();
        assert_eq!(c.model(Provider::OpenRouter), "google/gemma-4-31b-it:free");
        assert_eq!(c.model(Provider::Gemini), "x");
        let mut c: Config =
            serde_json::from_str(r#"{"models":{"openrouter":"meta/other:free"}}"#).unwrap();
        c.migrate();
        assert_eq!(c.model(Provider::OpenRouter), "meta/other:free");
    }

    #[test]
    fn old_plain_text_cache_entries_are_dropped() {
        let raw: HashMap<String, serde_json::Value> = serde_json::from_str(
            r#"{"old":"du texte brut","new":{"verdict":"watch","text":"ok"}}"#,
        )
        .unwrap();
        let e = Explanations::from_raw(raw);
        assert!(e.get("old").is_none());
        assert_eq!(e.get("New ×3").unwrap().verdict, Verdict::Watch);
    }
}
