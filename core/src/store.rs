//! Local config (API base URL) and token storage, shared by the `fxr` CLI
//! and the desktop app so logging in with either one works from both:
//! same config file path, same OS-keychain service/account.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::client::DEFAULT_BASE_URL;
use crate::error::{Error, Result};

const KEYCHAIN_SERVICE: &str = "fxrender-client";
const KEYCHAIN_ACCOUNT: &str = "default";
const CONFIG_DIR_NAME: &str = "fxrender-client";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_api_url")]
    pub api_url: String,
}

impl Default for Config {
    fn default() -> Self {
        Self { api_url: default_api_url() }
    }
}

fn default_api_url() -> String {
    DEFAULT_BASE_URL.to_string()
}

pub fn config_dir() -> Result<PathBuf> {
    let dir = dirs::config_dir()
        .ok_or_else(|| Error::Other("could not determine a config directory for this platform".into()))?
        .join(CONFIG_DIR_NAME);
    fs::create_dir_all(&dir).map_err(|e| Error::Other(format!("creating {}: {e}", dir.display())))?;
    Ok(dir)
}

fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.json"))
}

pub fn load_config() -> Result<Config> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(Config::default());
    }
    let text = fs::read_to_string(&path)
        .map_err(|e| Error::Other(format!("reading {}: {e}", path.display())))?;
    Ok(serde_json::from_str(&text).unwrap_or_default())
}

pub fn save_config(cfg: &Config) -> Result<()> {
    let path = config_path()?;
    let text = serde_json::to_string_pretty(cfg)?;
    fs::write(&path, text).map_err(|e| Error::Other(format!("writing {}: {e}", path.display())))
}

fn keychain_entry() -> Result<keyring::Entry> {
    keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)
        .map_err(|e| Error::Other(format!("opening system keychain: {e}")))
}

pub fn get_token() -> Result<Option<String>> {
    match keychain_entry()?.get_password() {
        Ok(token) => Ok(Some(token)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => {
            eprintln!("[fxrender-core] keychain get_password FAILED: {e:?}");
            Err(Error::Other(format!("reading token from keychain: {e}")))
        }
    }
}

pub fn set_token(token: &str) -> Result<()> {
    match keychain_entry()?.set_password(token) {
        Ok(()) => {
            eprintln!("[fxrender-core] keychain set_password: ok ({} char token)", token.len());
            Ok(())
        }
        Err(e) => {
            eprintln!("[fxrender-core] keychain set_password FAILED: {e:?}");
            Err(Error::Other(format!("saving token to keychain: {e}")))
        }
    }
}

pub fn delete_token() -> Result<()> {
    match keychain_entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {
            eprintln!("[fxrender-core] keychain delete_credential: ok");
            Ok(())
        }
        Err(e) => {
            eprintln!("[fxrender-core] keychain delete_credential FAILED: {e:?}");
            Err(Error::Other(format!("removing token from keychain: {e}")))
        }
    }
}

/// Convenience: build a [`crate::Client`] from the saved config + token, the
/// way both the CLI and the desktop app want to at startup.
pub fn load_client() -> Result<crate::Client> {
    let cfg = load_config()?;
    let token = get_token()?.ok_or(Error::MissingToken)?;
    crate::Client::new(cfg.api_url, token)
}
