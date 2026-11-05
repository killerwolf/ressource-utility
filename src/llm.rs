//! Client LLM minimal (Anthropic, Gemini, OpenAI) — bloquant, à appeler hors du thread principal.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fmt;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Anthropic,
    Gemini,
    OpenAi,
}

impl Provider {
    pub const ALL: [Provider; 3] = [Provider::Anthropic, Provider::Gemini, Provider::OpenAi];

    pub fn id(self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic",
            Provider::Gemini => "gemini",
            Provider::OpenAi => "openai",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Provider::Anthropic => "Anthropic",
            Provider::Gemini => "Gemini",
            Provider::OpenAi => "OpenAI (Codex)",
        }
    }

    pub fn default_model(self) -> &'static str {
        match self {
            Provider::Anthropic => "claude-haiku-5-5",
            Provider::Gemini => "gemini-2.5-flash",
            Provider::OpenAi => "gpt-5-mini",
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum LlmError {
    Http(u16),
    Network(String),
    Parse,
    Empty,
    NoKey,
}

impl fmt::Display for LlmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LlmError::Http(c @ (401 | 403)) => write!(f, "Clé API invalide ({c})"),
            LlmError::Http(404) => write!(f, "Modèle introuvable (404), vérifie le nom du modèle"),
            LlmError::Http(400) => write!(f, "Requête refusée (400), vérifie le modèle"),
            LlmError::Http(429) => write!(f, "Quota ou limite de débit dépassé (429)"),
            LlmError::Http(c) if *c >= 500 => write!(f, "Service IA indisponible ({c})"),
            LlmError::Http(c) => write!(f, "Erreur du service IA ({c})"),
            LlmError::Network(_) => write!(f, "Réseau indisponible ou délai dépassé"),
            LlmError::Parse => write!(f, "Réponse inattendue du service IA"),
            LlmError::Empty => write!(f, "Réponse vide du service IA"),
            LlmError::NoKey => write!(f, "Configure une clé API pour des conseils IA"),
        }
    }
}

impl std::error::Error for LlmError {}

/// Requête HTTP prête à l'envoi (pure, testable).
pub struct Request {
    pub url: String,
    pub headers: Vec<(&'static str, String)>,
    pub body: Value,
}

pub fn build_request(
    p: Provider,
    key: &str,
    model: &str,
    system: &str,
    user: &str,
    max_tokens: u32,
) -> Request {
    match p {
        Provider::Anthropic => Request {
            url: "https://api.anthropic.com/v1/messages".into(),
            headers: vec![
                ("x-api-key", key.to_string()),
                ("anthropic-version", "2023-06-01".into()),
            ],
            body: json!({
                "model": model,
                "max_tokens": max_tokens,
                "system": system,
                "messages": [{"role": "user", "content": user}],
            }),
        },
        Provider::Gemini => {
            let mut gen_cfg = json!({ "maxOutputTokens": max_tokens });
            // Les modèles « flash » réfléchissent par défaut : ces tokens de
            // réflexion consommeraient tout le budget de sortie.
            if model.contains("flash") {
                gen_cfg["thinkingConfig"] = json!({ "thinkingBudget": 0 });
            }
            Request {
                url: format!(
                    "https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent"
                ),
                headers: vec![("x-goog-api-key", key.to_string())],
                body: json!({
                    "systemInstruction": {"parts": [{"text": system}]},
                    "contents": [{"role": "user", "parts": [{"text": user}]}],
                    "generationConfig": gen_cfg,
                }),
            }
        }
        Provider::OpenAi => {
            let mut body = json!({
                "model": model,
                "messages": [
                    {"role": "system", "content": system},
                    {"role": "user", "content": user},
                ],
                "max_completion_tokens": max_tokens,
            });
            // Les modèles gpt-5 sont des modèles de raisonnement : on limite la
            // réflexion pour que le budget de sortie serve à la réponse.
            if model.starts_with("gpt-5") {
                body["reasoning_effort"] = json!("minimal");
            }
            Request {
                url: "https://api.openai.com/v1/chat/completions".into(),
                headers: vec![("Authorization", format!("Bearer {key}"))],
                body,
            }
        }
    }
}

pub fn parse_response(p: Provider, body: &Value) -> Result<String, LlmError> {
    let text = match p {
        Provider::Anthropic => body
            .pointer("/content/0/text")
            .and_then(Value::as_str)
            .map(str::to_owned),
        Provider::Gemini => body
            .pointer("/candidates/0/content/parts")
            .and_then(Value::as_array)
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|p| p.get("text").and_then(Value::as_str))
                    .collect::<String>()
            }),
        Provider::OpenAi => body
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .map(str::to_owned),
    };
    match text {
        None => Err(LlmError::Parse),
        Some(t) if t.trim().is_empty() => Err(LlmError::Empty),
        Some(t) => Ok(t.trim().to_string()),
    }
}

pub fn ask(
    p: Provider,
    key: &str,
    model: &str,
    system: &str,
    user: &str,
    max_tokens: u32,
) -> Result<String, LlmError> {
    let req = build_request(p, key, model, system, user, max_tokens);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .http_status_as_error(false)
        .build()
        .into();
    let mut r = agent.post(&req.url);
    for (k, v) in &req.headers {
        r = r.header(*k, v);
    }
    let mut resp = r
        .send_json(&req.body)
        .map_err(|e| LlmError::Network(e.to_string()))?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(LlmError::Http(status));
    }
    let json: Value = resp.body_mut().read_json().map_err(|_| LlmError::Parse)?;
    parse_response(p, &json)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(p: Provider, s: &str) -> Result<String, LlmError> {
        parse_response(p, &serde_json::from_str(s).unwrap())
    }

    #[test]
    fn parses_anthropic() {
        let s = r#"{"id":"msg_1","type":"message","content":[{"type":"text","text":" - Ferme Chrome\n"}],"usage":{"input_tokens":10,"output_tokens":5}}"#;
        assert_eq!(parse(Provider::Anthropic, s).unwrap(), "- Ferme Chrome");
    }

    #[test]
    fn parses_gemini() {
        let s = r#"{"candidates":[{"content":{"parts":[{"text":"Salut "},{"text":"toi"}],"role":"model"},"finishReason":"STOP"}]}"#;
        assert_eq!(parse(Provider::Gemini, s).unwrap(), "Salut toi");
    }

    #[test]
    fn parses_openai() {
        let s = r#"{"choices":[{"index":0,"message":{"role":"assistant","content":"Ok"},"finish_reason":"stop"}]}"#;
        assert_eq!(parse(Provider::OpenAi, s).unwrap(), "Ok");
    }

    #[test]
    fn rejects_bad_shapes() {
        assert_eq!(parse(Provider::Anthropic, "{}"), Err(LlmError::Parse));
        assert_eq!(
            parse(
                Provider::OpenAi,
                r#"{"choices":[{"message":{"content":""}}]}"#
            ),
            Err(LlmError::Empty)
        );
        assert_eq!(
            parse(
                Provider::Gemini,
                r#"{"candidates":[{"finishReason":"SAFETY"}]}"#
            ),
            Err(LlmError::Parse)
        );
    }

    #[test]
    fn error_messages_are_french() {
        assert_eq!(LlmError::Http(401).to_string(), "Clé API invalide (401)");
        assert!(LlmError::Http(429).to_string().contains("429"));
        assert!(LlmError::Http(503).to_string().contains("indisponible"));
    }

    #[test]
    fn requests_have_expected_shape() {
        let r = build_request(Provider::Anthropic, "k", "m", "sys", "usr", 200);
        assert_eq!(r.body["max_tokens"], 200);
        assert_eq!(r.body["messages"][0]["content"], "usr");
        assert!(r.headers.iter().any(|(h, _)| *h == "x-api-key"));

        let r = build_request(Provider::Gemini, "k", "gemini-2.5-flash", "sys", "usr", 200);
        assert!(r.url.ends_with("gemini-2.5-flash:generateContent"));
        assert_eq!(r.body["generationConfig"]["maxOutputTokens"], 200);
        assert_eq!(r.body["systemInstruction"]["parts"][0]["text"], "sys");
        let r = build_request(Provider::Gemini, "k", "gemini-2.5-pro", "s", "u", 200);
        assert!(r.body["generationConfig"].get("thinkingConfig").is_none());

        let r = build_request(Provider::OpenAi, "k", "gpt-5-mini", "sys", "usr", 200);
        assert_eq!(r.body["max_completion_tokens"], 200);
        assert_eq!(r.body["messages"][0]["role"], "system");
        assert_eq!(r.headers[0].1, "Bearer k");
    }
}
