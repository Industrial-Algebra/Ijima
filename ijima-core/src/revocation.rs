// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! Token revocation — the kill-switch for issued grant tokens.
//!
//! Schubert `GrantToken`s are stateless Ed25519 signatures: valid until
//! the heat death of the key. For a long-lived multi-principal deployment
//! that is unacceptable — a leaked bearer must be killable *now*, not at
//! the next issuer-key rotation.
//!
//! Ijima's answer is a store-backed **revocation list**: the daemon keeps
//! an in-memory hash set (hydrated from the store at boot, appended via
//! the admin route) and rejects any bearer whose SHA-256 hash is a
//! member — checked right after the cryptographic verify, so a revoked
//! token is exactly as dead as a bad-signature token.
//!
//! **Why a hash, not the token?** Revocation entries may be inspected by
//! operators or synced to satellites; storing raw bearers would leak
//! live credentials into logs/backups. The SHA-256 of the bearer leaks
//! nothing usable.
//!
//! **Why not expiry?** Token-carried expiry belongs in Schubert
//! (`GrantToken` fields + verify-time check, requested for Schubert 0.5).
//! Revocation and expiry are complementary: expiry handles routine
//! deprovisioning; revocation handles incidents. See
//! `docs/adr/token-revocation.md`.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// A revoked grant token, identified by the SHA-256 hex of its bearer
/// string.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TokenRevocation {
    /// SHA-256 hex digest of the revoked bearer token (primary key).
    pub token_hash: String,
    /// When the revocation was recorded (unix seconds).
    pub revoked_at_unix: u64,
    /// Operator note — e.g. `"leaked in CI log"` (optional).
    pub reason: Option<String>,
}
