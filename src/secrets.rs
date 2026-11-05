//! Clés API dans le Trousseau macOS. Jamais écrites sur disque ni dans les logs.

use crate::llm::Provider;
use keyring::Entry;

const SERVICE: &str = "com.ressource-utility";

fn entry(p: Provider) -> Result<Entry, String> {
    Entry::new(SERVICE, p.id()).map_err(|e| format!("Trousseau indisponible : {e}"))
}

pub fn get(p: Provider) -> Option<String> {
    entry(p)
        .ok()?
        .get_password()
        .ok()
        .filter(|k| !k.trim().is_empty())
}

pub fn delete(p: Provider) -> Result<(), String> {
    match entry(p)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("Impossible de supprimer la clé : {e}")),
    }
}

pub fn set(p: Provider, key: &str) -> Result<(), String> {
    entry(p)?
        .set_password(key.trim())
        .map_err(|e| format!("Impossible d'enregistrer la clé : {e}"))
}
