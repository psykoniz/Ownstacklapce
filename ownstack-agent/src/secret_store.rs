//! Secret store helper for provider API keys.
//!
//! Synchronizes environment variables with the OS keyring:
//! - If env has a key and keyring is missing/outdated -> write to keyring.
//! - If env is missing and keyring has a key -> hydrate env from keyring.

use tracing::{debug, info, warn};

const KEYRING_SERVICE: &str = "OwnStack IDE";

#[derive(Clone, Copy)]
struct SecretSpec {
    env_var: &'static str,
    entry_name: &'static str,
}

const SECRET_SPECS: &[SecretSpec] = &[
    SecretSpec {
        env_var: "OPENROUTER_API_KEY",
        entry_name: "openrouter_api_key",
    },
    SecretSpec {
        env_var: "ANTHROPIC_API_KEY",
        entry_name: "anthropic_api_key",
    },
    SecretSpec {
        env_var: "OPENAI_API_KEY",
        entry_name: "openai_api_key",
    },
];

fn spec_for_env_var(env_var: &str) -> Option<SecretSpec> {
    SECRET_SPECS.iter().copied().find(|s| s.env_var == env_var)
}

fn read_keyring(entry_name: &str) -> Option<String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, entry_name).ok()?;
    let value = entry.get_password().ok()?;
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

fn write_keyring(entry_name: &str, value: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, entry_name)
        .map_err(|e| format!("keyring entry error: {e}"))?;
    entry
        .set_password(value)
        .map_err(|e| format!("keyring write error: {e}"))
}

pub fn get_secret(env_var: &str) -> Option<String> {
    if let Ok(value) = std::env::var(env_var) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    let spec = spec_for_env_var(env_var)?;
    read_keyring(spec.entry_name)
}

pub fn has_secret(env_var: &str) -> bool {
    get_secret(env_var).is_some()
}

pub fn set_secret(env_var: &str, value: &str) -> Result<(), String> {
    let spec = spec_for_env_var(env_var)
        .ok_or_else(|| format!("unsupported secret env var: {env_var}"))?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("secret must not be empty".to_string());
    }
    write_keyring(spec.entry_name, trimmed)?;
    std::env::set_var(env_var, trimmed);
    Ok(())
}

pub fn sync_env_and_keyring() {
    for spec in SECRET_SPECS {
        sync_one(spec);
    }
}

fn sync_one(spec: &SecretSpec) {
    let env_value = std::env::var(spec.env_var)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let keyring_value = read_keyring(spec.entry_name);

    match (env_value, keyring_value) {
        (Some(env), Some(stored)) => {
            if env != stored {
                if let Err(err) = write_keyring(spec.entry_name, &env) {
                    warn!(
                        "Failed to update keyring entry '{}' from env '{}': {}",
                        spec.entry_name, spec.env_var, err
                    );
                } else {
                    info!(
                        "Updated keyring entry '{}' from env '{}'",
                        spec.entry_name, spec.env_var
                    );
                }
            }
        }
        (Some(env), None) => {
            if let Err(err) = write_keyring(spec.entry_name, &env) {
                warn!(
                    "Failed to persist env '{}' into keyring '{}': {}",
                    spec.env_var, spec.entry_name, err
                );
            } else {
                info!(
                    "Persisted env '{}' into keyring entry '{}'",
                    spec.env_var, spec.entry_name
                );
            }
        }
        (None, Some(stored)) => {
            std::env::set_var(spec.env_var, &stored);
            debug!(
                "Hydrated env '{}' from keyring entry '{}'",
                spec.env_var, spec.entry_name
            );
        }
        (None, None) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_secret_name_is_rejected() {
        let result = set_secret("UNKNOWN_SECRET", "abc");
        assert!(result.is_err());
    }

    #[test]
    fn empty_secret_is_rejected() {
        let result = set_secret("OPENROUTER_API_KEY", "   ");
        assert!(result.is_err());
    }
}
