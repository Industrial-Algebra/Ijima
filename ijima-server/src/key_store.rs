// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! Persistence for the Ijima issuer key seed.
//!
//! The Schubert capability tokens Ijima mints are signed by a single
//! Ed25519 issuer key. For the daemon and the `ijima token` CLI to agree,
//! they must share the same 32-byte seed. This module loads it from
//! `$IJIMA_DIR/issuer.key` (default `~/.ijima/issuer.key`), creating it
//! with a fresh random seed on first use, mode `0600`.

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
    if let Ok(dir) = std::env::var("IJIMA_DIR") {
        return Ok(PathBuf::from(dir).join(KEY_FILENAME));
    }
    let home = home_dir().ok_or_else(|| {
        IjimaError::invalid_input("cannot resolve Ijima data dir: set IJIMA_DIR or HOME")
    })?;
    Ok(home.join(".ijima").join(KEY_FILENAME))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Loads the seed at `path`, or creates it with a fresh random value if
/// absent (mode `0600` on Unix). Used by both the daemon (first start)
/// and the `ijima token issue` CLI.
///
/// # Errors
///
/// Returns [`IjimaError::Store`] on I/O failure or
/// [`IjimaError::InvalidInput`] if an existing file is not 32 bytes.
pub fn load_or_create(path: &Path) -> Result<[u8; 32]> {
    match std::fs::read(path) {
        Ok(bytes) => seed_from_bytes(&bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let seed = crate::IjimaAuth::generate_seed();
            write_seed(path, &seed)?;
            Ok(seed)
        }
        Err(e) => Err(IjimaError::Store {
            detail: format!("read {}: {e}", path.display()),
        }),
    }
}

/// Loads an existing seed without creating one.
///
/// # Errors
///
/// Returns [`IjimaError::Store`] on I/O failure or
/// [`IjimaError::InvalidInput`] if the file is absent or not 32 bytes.
pub fn load(path: &Path) -> Result<[u8; 32]> {
    match std::fs::read(path) {
        Ok(bytes) => seed_from_bytes(&bytes),
        Err(e) => Err(IjimaError::Store {
            detail: format!("read {}: {e}", path.display()),
        }),
    }
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

fn seed_from_bytes(bytes: &[u8]) -> Result<[u8; 32]> {
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| IjimaError::invalid_input("issuer key file must be exactly 32 bytes"))?;
    Ok(arr)
}

fn write_seed(path: &Path, seed: &[u8; 32]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| IjimaError::Store {
            detail: format!("mkdir {}: {e}", parent.display()),
        })?;
    }
    write_secret_file(path, seed).map_err(|e| IjimaError::Store {
        detail: format!("write {}: {e}", path.display()),
    })?;
    #[cfg(unix)]
    set_owner_only(path)?;
    Ok(())
}

#[cfg(unix)]
fn set_owner_only(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(|e| {
        IjimaError::Store {
            detail: format!("chmod {}: {e}", path.display()),
        }
    })?;
    Ok(())
}

// On non-Unix we still write the file; POSIX permissions are best-effort.
#[cfg(not(unix))]
fn write_secret_file(path: &Path, seed: &[u8; 32]) -> std::io::Result<()> {
    std::fs::write(path, seed)
}

#[cfg(unix)]
fn write_secret_file(path: &Path, seed: &[u8; 32]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(seed)
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
