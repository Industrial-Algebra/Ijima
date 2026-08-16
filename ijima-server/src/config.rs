// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! Layered configuration for the Ijima daemon and CLI.
//!
//! Resolution order, lowest to highest:
//!
//! ```text
//! built-in defaults  <  ijima.toml (file)  <  env vars  <  CLI flags
//! ```
//!
//! ## File discovery
//!
//! The first existing path wins:
//!
//! 1. `$IJIMA_CONFIG` (explicit path — errors if unreadable/malformed)
//! 2. `$IJIMA_DIR/ijima.toml` (or `~/.ijima/ijima.toml`)
//! 3. `/etc/ijima/ijima.toml`
//!
//! Unknown keys in the file are ignored (forward compatibility). An
//! explicit `$IJIMA_CONFIG` that cannot be parsed is a hard error — a
//! deployment that points at a config expects it to be honored.
//!
//! ## Example
//!
//! ```toml
//! # /etc/ijima/ijima.toml
//! host = "127.0.0.1"
//! port = 7373
//! data_dir = "/var/lib/ijima"
//! issuer_key = "/var/lib/ijima/issuer.key"
//! rate_base = 10.0
//! rate_multiplier = 1.0
//! embedding_model = "sentence-transformers/all-MiniLM-L6-v2"
//! ```

use std::path::{Path, PathBuf};

use ijima_core::{IjimaError, Result};
use serde::Deserialize;

/// The file layer of Ijima configuration (`ijima.toml`). Every field is
/// optional — absent fields fall through to env/defaults.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct IjimaToml {
    /// Bind host (env `IJIMA_HOST`, default `127.0.0.1`).
    pub host: Option<String>,
    /// Bind port (env `IJIMA_PORT`, default `7373`).
    pub port: Option<u16>,
    /// Data directory — where the SurrealDB store and issuer key live
    /// (env `IJIMA_DIR`, default `~/.ijima`).
    pub data_dir: Option<String>,
    /// Issuer key path (default `<data_dir>/issuer.key`).
    pub issuer_key: Option<String>,
    /// Rate-limit base tokens/sec per intersection-number unit
    /// (env `IJIMA_RATE_BASE`, default `10`).
    pub rate_base: Option<f64>,
    /// Rate-limit multiplier (env `IJIMA_RATE_MULTIPLIER`, default `1.0`).
    pub rate_multiplier: Option<f64>,
    /// Hugging Face model id for candle embeddings
    /// (default `sentence-transformers/all-MiniLM-L6-v2`).
    pub embedding_model: Option<String>,
}

/// Returns the config file path if one exists, per the discovery order.
///
/// `$IJIMA_CONFIG` wins even if the file does not exist (an explicit
/// pointer means the operator expects it); the implicit fallbacks only
/// apply when it is unset.
pub fn discover_path() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("IJIMA_CONFIG") {
        return Some(PathBuf::from(explicit));
    }
    if let Some(dir) = data_dir_implicit() {
        let p = dir.join("ijima.toml");
        if p.exists() {
            return Some(p);
        }
    }
    let etc = Path::new("/etc/ijima/ijima.toml");
    etc.exists().then(|| etc.to_path_buf())
}

/// Loads and parses the config file, if any exists.
///
/// # Errors
///
/// - [`IjimaError::InvalidInput`] if an existing file cannot be read or
///   parsed (including an explicit `$IJIMA_CONFIG` pointing at a missing
///   file — an explicit pointer must be honored, not silently skipped).
pub fn load() -> Result<IjimaToml> {
    let Some(path) = discover_path() else {
        return Ok(IjimaToml::default());
    };
    let text = std::fs::read_to_string(&path).map_err(|e| {
        IjimaError::invalid_input(format!("config file {} unreadable: {e}", path.display()))
    })?;
    toml::from_str(&text).map_err(|e| {
        IjimaError::invalid_input(format!("config file {} malformed: {e}", path.display()))
    })
}

/// The implicit data dir (no env, no file layer): `$IJIMA_DIR` or
/// `~/.ijima`. Used for file discovery before the file is parsed.
fn data_dir_implicit() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("IJIMA_DIR") {
        return Some(PathBuf::from(dir));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".ijima"))
}

/// Resolves the data directory: env `IJIMA_DIR` > config file `data_dir`
/// > `~/.ijima`.
///
/// # Errors
///
/// Returns [`IjimaError::InvalidInput`] if neither env var, config file,
/// nor `HOME` can resolve a directory.
pub fn resolve_data_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("IJIMA_DIR") {
        return Ok(PathBuf::from(dir));
    }
    if let Some(dir) = load().ok().and_then(|c| c.data_dir) {
        return Ok(PathBuf::from(dir));
    }
    data_dir_implicit()
        .ok_or_else(|| IjimaError::invalid_input("cannot resolve data dir: set IJIMA_DIR or HOME"))
}

/// Resolves a string setting: env var > config field > default.
pub fn resolve_str(env_key: &str, file: Option<String>, default: &str) -> String {
    std::env::var(env_key).unwrap_or_else(|_| file.unwrap_or_else(|| default.to_string()))
}

/// Resolves an optional-path setting: env var > config field > `None`.
pub fn resolve_path(env_key: &str, file: Option<String>) -> Option<PathBuf> {
    std::env::var_os(env_key)
        .map(PathBuf::from)
        .or_else(|| file.map(PathBuf::from))
}

/// Resolves an `f64` setting: env var > config field > default. Malformed
/// env values fall through to the next layer.
pub fn resolve_f64(env_key: &str, file: Option<f64>, default: f64) -> f64 {
    std::env::var(env_key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(file.unwrap_or(default))
}

// SAFETY Justification for `allow(unsafe_code)` in tests: Rust 2024
// marks `set_var`/`remove_var` unsafe because unsynchronized env mutation
// from multiple threads is UB. Every env mutation here happens inside
// `with_env`, whose static mutex serializes all such tests; other tests in
// this crate never read these three vars. Sound under that invariant.
#[cfg(test)]
#[allow(unsafe_code)]
mod tests {
    use super::*;

    /// Runs `f` with `IJIMA_CONFIG`/`IJIMA_DIR`/`HOME` isolated: saved &
    /// removed before, restored after. The env-mutex guard lives in this
    /// function's frame for the whole test body, serializing all
    /// env-touching tests (they run in parallel otherwise).
    fn with_env(f: impl FnOnce(&EnvVars)) {
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        const VARS: [&str; 3] = ["IJIMA_CONFIG", "IJIMA_DIR", "HOME"];
        let saved: Vec<Option<String>> = VARS.iter().map(|k| std::env::var(k).ok()).collect();
        // SAFETY: serialized by ENV_LOCK (see module comment).
        for k in VARS {
            unsafe { std::env::remove_var(k) };
        }
        let env = EnvVars;
        f(&env);
        // SAFETY: serialized by ENV_LOCK (see module comment).
        for (k, v) in VARS.iter().zip(saved) {
            match v {
                Some(val) => unsafe { std::env::set_var(k, val) },
                None => unsafe { std::env::remove_var(k) },
            }
        }
    }

    /// Handle for setting isolated env vars inside [`with_env`].
    struct EnvVars;

    impl EnvVars {
        fn set(&self, key: &str, value: &str) {
            // SAFETY: serialized by with_env's ENV_LOCK (see module comment).
            unsafe { std::env::set_var(key, value) };
        }
    }

    fn write_config(dir: &Path, body: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let p = dir.join("ijima.toml");
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn no_config_anywhere_yields_defaults() {
        with_env(|_| {
            assert_eq!(load().unwrap(), IjimaToml::default());
        });
    }

    #[test]
    fn explicit_config_env_wins_and_parses() {
        with_env(|env| {
            let dir = std::env::temp_dir().join(format!("ijima-cfg-{}", std::process::id()));
            let p = write_config(&dir, "host = \"0.0.0.0\"\nport = 8000\nrate_base = 5.5\n");
            env.set("IJIMA_CONFIG", p.to_str().unwrap());

            let cfg = load().unwrap();
            assert_eq!(cfg.host.as_deref(), Some("0.0.0.0"));
            assert_eq!(cfg.port, Some(8000));
            assert_eq!(cfg.rate_base, Some(5.5));
            assert_eq!(cfg.data_dir, None); // unset fields stay None
        });
    }

    #[test]
    fn unknown_keys_are_ignored() {
        with_env(|env| {
            let dir = std::env::temp_dir().join(format!("ijima-cfg2-{}", std::process::id()));
            let p = write_config(&dir, "future_key = true\nhost = \"h\"\n");
            env.set("IJIMA_CONFIG", p.to_str().unwrap());
            assert_eq!(load().unwrap().host.as_deref(), Some("h"));
        });
    }

    #[test]
    fn malformed_config_is_a_hard_error() {
        with_env(|env| {
            let dir = std::env::temp_dir().join(format!("ijima-cfg3-{}", std::process::id()));
            let p = write_config(&dir, "port = [not a number");
            env.set("IJIMA_CONFIG", p.to_str().unwrap());
            assert!(load().is_err());
        });
    }

    #[test]
    fn explicit_config_missing_file_is_an_error() {
        with_env(|env| {
            env.set("IJIMA_CONFIG", "/nonexistent/ijima.toml");
            assert!(load().is_err());
        });
    }

    #[test]
    fn implicit_config_in_ijima_dir_is_discovered() {
        with_env(|env| {
            let dir = std::env::temp_dir().join(format!("ijima-cfg4-{}", std::process::id()));
            write_config(&dir, "host = \"file-host\"\n");
            env.set("IJIMA_DIR", dir.to_str().unwrap());
            assert_eq!(load().unwrap().host.as_deref(), Some("file-host"));
        });
    }

    #[test]
    fn data_dir_env_beats_file_beats_home() {
        with_env(|env| {
            let dir = std::env::temp_dir().join(format!("ijima-cfg5-{}", std::process::id()));
            let p = write_config(&dir, "data_dir = \"/from-file\"\n");
            env.set("IJIMA_CONFIG", p.to_str().unwrap());
            env.set("HOME", "/home/tester");

            // file layer beats home
            assert_eq!(resolve_data_dir().unwrap(), PathBuf::from("/from-file"));

            // env layer beats file
            env.set("IJIMA_DIR", "/from-env");
            assert_eq!(resolve_data_dir().unwrap(), PathBuf::from("/from-env"));
        });
    }

    #[test]
    fn data_dir_home_fallback() {
        with_env(|env| {
            env.set("HOME", "/home/tester");
            assert_eq!(
                resolve_data_dir().unwrap(),
                PathBuf::from("/home/tester/.ijima")
            );
        });
    }

    #[test]
    fn resolve_f64_env_beats_file_beats_default() {
        assert_eq!(resolve_f64("IJIMA_NOPE_XYZ", Some(2.5), 1.0), 2.5);
        assert_eq!(resolve_f64("IJIMA_NOPE_XYZ", None, 1.0), 1.0);
        // malformed env falls through to file. SAFETY: unique env key
        // touched only by this test; other tests never read it.
        unsafe { std::env::set_var("IJIMA_NOPE_XYZ", "not-a-number") };
        assert_eq!(resolve_f64("IJIMA_NOPE_XYZ", Some(2.5), 1.0), 2.5);
        unsafe { std::env::remove_var("IJIMA_NOPE_XYZ") };
    }
}
