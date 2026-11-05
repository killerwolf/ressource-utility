//! Configuration (non secrète) et cache d'explications, en JSON.

use crate::anomaly::{Gate, Thresholds};
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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: Provider::Anthropic,
            models: HashMap::new(),
            thresholds: Thresholds::default(),
            launch_at_login: false,
            gate: Gate::default(),
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
        load_json("config.json")
    }

    pub fn save(&self) {
        save_json("config.json", self);
    }
}

/// Explications de processus, mises en cache à vie par nom.
#[derive(Default, Serialize, Deserialize)]
pub struct Explanations(pub HashMap<String, String>);

impl Explanations {
    pub fn load() -> Self {
        load_json("explanations.json")
    }

    pub fn get(&self, name: &str) -> Option<&String> {
        self.0.get(&name.to_lowercase())
    }

    pub fn insert(&mut self, name: &str, text: String) {
        self.0.insert(name.to_lowercase(), text);
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
}
