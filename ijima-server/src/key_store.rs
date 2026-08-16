// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! Persistence for the Ijima issuer key seed.
//!
//! The Schubert capability tokens Ijima mints are signed by a single
//! Ed25519 issuer key. For the daemon and the `ijima token` CLI to agree,
//! they must share the same 32-byte seed. This module resolves the path
//! (`$IJIMA_DIR/issuer.key`, default `~/.ijima/issuer.key`) and delegates
//! load/create to [`schubert::crypto::KeyStore`] (upstream since Schubert
//! v0.4 — Ijima no longer reimplements the file/permission logic).

use std::path::{Path, PathBuf};

use ijima_core::{IjimaError, Result};

const KEY_FILENAME: &str = "issuer.key";

/// Resolves the issuer key path: `$IJIMA_DIR/issuer.key`, else
/// `$HOME/.ijima/issuer.key`.
///
/// # Errors
///
/// Returns [`IjimaError::InvalidInput`] if `IJIMA_DIR` is unset and the
/// home directory cannot be determined.
pub fn default_key_path() -> Result<PathBuf> {
    // Env IJIMA_KEY / config `issuer_key` override the data-dir default;
    // the data dir itself resolves env > file > ~/.ijima (see `config`).
    let file = crate::config::load().ok();
    if let Some(p) = crate::config::resolve_path("IJIMA_KEY", file.and_then(|c| c.issuer_key)) {
        return Ok(p);
    }
    Ok(crate::config::resolve_data_dir()?.join(KEY_FILENAME))
}

/// Loads the seed at `path`, or creates it with a fresh random value if
/// absent (mode `0600` on Unix). Used by both the daemon (first start)
/// and the `ijima token issue` CLI. Delegates to
/// [`schubert::crypto::KeyStore::load_or_create`].
///
/// # Errors
///
/// Returns [`IjimaError::Store`] on I/O failure or
/// [`IjimaError::InvalidInput`] if an existing file is not 32 bytes.
pub fn load_or_create(path: &Path) -> Result<[u8; 32]> {
    schubert::crypto::KeyStore::load_or_create(path).map_err(|e| IjimaError::Store {
        detail: format!("issuer key {}: {e}", path.display()),
    })
}

/// Loads an existing seed without creating one. Delegates to
/// [`schubert::crypto::KeyStore::load`].
///
/// # Errors
///
/// Returns [`IjimaError::Store`] on I/O failure or
/// [`IjimaError::InvalidInput`] if the file is absent or not 32 bytes.
pub fn load(path: &Path) -> Result<[u8; 32]> {
    schubert::crypto::KeyStore::load(path).map_err(|e| IjimaError::Store {
        detail: format!("issuer key {}: {e}", path.display()),
    })
}

/// Formats a seed's derived public key as lowercase hex (for display).
pub fn public_key_hex(seed: &[u8; 32]) -> String {
    use ed25519_dalek::SigningKey;
    SigningKey::from_bytes(seed)
        .verifying_key()
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_or_create_then_load_round_trips() {
        let dir = tempfile_dir();
        let path = dir.join(KEY_FILENAME);
        let created = load_or_create(&path).expect("create");
        // Second call must read the same seed, not regenerate.
        let loaded = load_or_create(&path).expect("load");
        assert_eq!(created, loaded);
        let direct = load(&path).expect("direct load");
        assert_eq!(created, direct);
    }

    #[test]
    fn public_key_hex_is_64_lowercase_chars() {
        let seed = [1u8; 32];
        let hex = public_key_hex(&seed);
        assert_eq!(hex.len(), 64);
        assert!(
            hex.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
    }

    #[test]
    fn wrong_size_key_file_rejected() {
        let dir = tempfile_dir();
        let path = dir.join(KEY_FILENAME);
        std::fs::write(&path, b"too short").unwrap();
        assert!(load(&path).is_err());
    }

    fn tempfile_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ijima-keystore-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
