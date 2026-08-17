// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! Authentication + authorization via Schubert proof-carrying
//! **multi-capability grant tokens**.
//!
//! Per `docs/DESIGN.md` D4 and the GrantToken-migration ADR
//! (`docs/adr/grant-token-migration.md`), Ijima consumes Schubert 0.4's
//! [`GrantToken`] end-to-end: one signed token carries several
//! capabilities, each with its Schubert partition, and authorization is a
//! **geometric containment** check (`cap_partition ≤ granted_partition`,
//! component-wise) that is self-contained in the signed token — no
//! capability registry is consulted for the authz *decision*, only for the
//! static required-capability → partition lookup.
//!
//! Consequences of the geometry:
//! - **Write implies read** — `[2] ≥ [1]`, so a `memory:write` grant also
//!   satisfies `memory:read`.
//! - **Admin implies all** — `[4,4,4,4]` (the point class on Gr(4,8)) is
//!   ≥ every partition. The legacy `== "admin"` string short-circuit is
//!   gone; admin falls out of the geometry.
//!
//! The wire format is Schubert's native
//! [`GrantToken::to_bytes`](schubert::crypto::GrantToken::to_bytes) /
//! [`from_bytes`](schubert::crypto::GrantToken::from_bytes), base64-encoded
//! for bearer transport. Ijima no longer ships its own token serializer.

use std::sync::Arc;

use base64::{Engine, engine::general_purpose::STANDARD as B64};
use ijima_core::{IjimaError, Result, TokenRevocation};
use schubert::{
    AccessController, CapabilityId, PrincipalId,
    crypto::{CapabilityIssuer, GrantToken, GrantVerifier, KeyStore},
};
use sha2::{Digest, Sha256};

/// Ijima's Schubert policy, embedded at compile time.
const POLICY_TOML: &str = include_str!("../policy/policy.toml");

/// Computes the revocation key for a bearer: SHA-256 hex of the trimmed
/// bearer string, with an optional `Bearer ` scheme prefix stripped — so
/// operators (and CLIs) can paste either the raw token or the full
/// `Authorization` header value and hit the same revocation key. Storing
/// hashes instead of raw bearers keeps live credentials out of store
/// dumps, logs, and backups.
pub fn bearer_hash(bearer: &str) -> String {
    let trimmed = bearer.trim();
    let raw = trimmed.strip_prefix("Bearer ").unwrap_or(trimmed);
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    // sha2 0.11's `finalize()` output is not `{:x}`-formattable; hex it
    // byte-by-byte (same lowercase encoding as before).
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The authenticated principal + the verified grant carried by a bearer
/// token.
///
/// Produced by [`IjimaAuth::verify_bearer`]. Handlers consult
/// [`may`](Self::may) to enforce a specific capability. The grant's
/// partitions are cryptographically signed, so the geometric containment
/// check needs only the shared controller (to resolve a *required*
/// capability's partition) and the shared verifier — both cheap `Arc`
/// clones.
#[derive(Debug, Clone)]
pub struct AuthenticatedPrincipal {
    /// The principal this grant was issued to.
    pub principal: PrincipalId,
    /// The verified multi-capability grant.
    pub grant: GrantToken,
    controller: Arc<AccessController>,
    grant_verifier: Arc<GrantVerifier>,
}

impl AuthenticatedPrincipal {
    /// Returns true if the grant geometrically implies `required`
    /// (directly or because some granted partition `λ` satisfies
    /// `required.partition ≤ λ` component-wise).
    ///
    /// An unknown `required` capability (not in the policy) is denied.
    pub fn may(&self, required: &str) -> bool {
        match self.controller.capability(required) {
            Some(cap) => self.grant_verifier.may(&self.grant, &cap.partition),
            None => false,
        }
    }

    /// The list of capabilities explicitly carried by this grant (for
    /// debugging / operator visibility). Note this is the *signed* set,
    /// not the geometric closure — a grant of `[memory:write]` also implies
    /// `memory:read` via [`may`](Self::may) even though `read` is not
    /// listed here.
    pub fn granted_capabilities(&self) -> Vec<String> {
        self.grant
            .capabilities
            .iter()
            .map(|c| c.id.as_str().to_string())
            .collect()
    }

    /// This principal's default personal namespace id
    /// (`ns_<principal>_private`). Every request is scoped to this
    /// namespace unless explicit namespace parameters land later.
    pub fn personal_namespace(&self) -> ijima_core::NamespaceId {
        ijima_core::NamespaceId::new(format!("ns_{}_private", self.principal.as_str()))
    }
}

/// Ijima's auth core: an [`AccessController`] (capability → partition
/// resolver) plus a capability issuer and a grant verifier sharing one
/// Ed25519 key, **and an in-memory revocation set** (the grant
/// kill-switch — see `docs/adr/token-revocation.md`).
///
/// A daemon constructs one of these at startup, hydrates the revocation
/// set from the store
/// ([`hydrate_revocations`](Self::hydrate_revocations)), and serves; an
/// admin CLI uses the issuer to mint grant tokens via
/// [`IjimaAuth::issue_grant_bearer`].
#[derive(Debug)]
pub struct IjimaAuth {
    controller: Arc<AccessController>,
    issuer: CapabilityIssuer,
    grant_verifier: Arc<GrantVerifier>,
    /// SHA-256 hashes of revoked bearers. Mutex (not RwLock): the critical
    /// section is a hash-set lookup, too short to be worth reader
    /// parallelism.
    revocations: std::sync::Mutex<std::collections::HashSet<String>>,
}

impl IjimaAuth {
    /// Loads the embedded `policy/policy.toml` and generates a fresh
    /// Ed25519 issuer key pair.
    ///
    /// Use only for tests/ephemeral runs — every call produces a new key,
    /// so issued tokens will not verify against a different instance. For
    /// a persistent daemon/CLI, use
    /// [`from_embedded_policy_with_seed`](Self::from_embedded_policy_with_seed)
    /// with a seed from [`key_store`](crate::key_store).
    ///
    /// # Errors
    ///
    /// Returns [`IjimaError::InvalidInput`] if the policy TOML is invalid.
    pub fn from_embedded_policy() -> Result<Self> {
        Self::from_embedded_policy_with_seed(Self::generate_seed())
    }

    /// Loads the embedded policy and constructs the issuer from a known
    /// 32-byte Ed25519 seed. The same seed must be shared by every process
    /// that issues or verifies tokens for this Ijima instance.
    ///
    /// # Errors
    ///
    /// Returns [`IjimaError::InvalidInput`] if the policy TOML is invalid.
    pub fn from_embedded_policy_with_seed(seed: [u8; 32]) -> Result<Self> {
        let controller = AccessController::from_policy_toml(POLICY_TOML)
            .map_err(|e| IjimaError::invalid_input(format!("policy load: {e}")))?;
        let issuer = CapabilityIssuer::from_seed(seed);
        let grant_verifier = GrantVerifier::new(issuer.public_key());
        Ok(Self {
            controller: Arc::new(controller),
            issuer,
            grant_verifier: Arc::new(grant_verifier),
            revocations: std::sync::Mutex::new(std::collections::HashSet::new()),
        })
    }

    /// Generates a fresh random 32-byte issuer seed (for first-time setup).
    ///
    /// Delegates to [`KeyStore::generate_seed`](schubert::crypto::KeyStore::generate_seed).
    pub fn generate_seed() -> [u8; 32] {
        KeyStore::generate_seed()
    }

    /// The issuer's Ed25519 public key as lowercase hex, for distribution
    /// to verifiers and operator visibility.
    pub fn issuer_public_key_hex(&self) -> String {
        self.issuer.public_key_hex()
    }

    /// Returns the Grassmannian the controller operates on.
    pub fn grassmannian(&self) -> (usize, usize) {
        self.controller.grassmannian()
    }

    /// Issues a multi-capability grant token (base64 wire format) granting
    /// every capability in `capabilities` to `principal`.
    ///
    /// Each capability's partition is resolved from the embedded policy;
    /// an unknown capability is rejected. Singleton grants are issued with
    /// [`issue_bearer`](Self::issue_bearer).
    ///
    /// # Errors
    ///
    /// Returns [`IjimaError::InvalidInput`] if a capability is unknown to
    /// the policy, the grant is empty, or Schubert's issuer rejects the
    /// inputs.
    pub fn issue_grant_bearer(
        &self,
        principal: impl Into<PrincipalId>,
        capabilities: &[&str],
    ) -> Result<String> {
        if capabilities.is_empty() {
            return Err(IjimaError::invalid_input(
                "grant must carry at least one capability",
            ));
        }
        let mut entries = Vec::with_capacity(capabilities.len());
        for cap in capabilities {
            let partition = self
                .controller
                .capability(cap)
                .map(|c| c.partition.clone())
                .ok_or_else(|| IjimaError::invalid_input(format!("unknown capability: {cap}")))?;
            entries.push((CapabilityId::new(*cap), partition));
        }
        let grant = self
            .issuer
            .issue_grant(principal, &entries)
            .map_err(|e| IjimaError::invalid_input(format!("grant issue: {e}")))?;
        Ok(B64.encode(GrantToken::to_bytes(&grant)))
    }

    /// Convenience: issues a single-capability grant. Equivalent to
    /// [`issue_grant_bearer`](Self::issue_grant_bearer) with one entry.
    ///
    /// # Errors
    ///
    /// Returns [`IjimaError::InvalidInput`] if the capability is unknown or
    /// Schubert's issuer rejects the inputs.
    pub fn issue_bearer(
        &self,
        principal: impl Into<PrincipalId>,
        capability: impl AsRef<str>,
    ) -> Result<String> {
        self.issue_grant_bearer(principal, &[capability.as_ref()])
    }

    /// Hydrates the in-memory revocation set from store-backed records
    /// (daemon boot). Replaces any prior set.
    pub fn hydrate_revocations(&self, revocations: &[TokenRevocation]) {
        let mut set = self.revocations.lock().expect("revocations poisoned");
        *set = revocations.iter().map(|r| r.token_hash.clone()).collect();
    }

    /// Adds a revocation to the in-memory set (after the store write — the
    /// admin route persists first, then calls this). Idempotent.
    pub fn revoke(&self, hash: &str) {
        self.revocations
            .lock()
            .expect("revocations poisoned")
            .insert(hash.to_string());
    }

    /// True if the bearer's hash is revoked.
    pub fn is_revoked(&self, bearer: &str) -> bool {
        self.revocations
            .lock()
            .expect("revocations poisoned")
            .contains(&bearer_hash(bearer))
    }

    /// Decodes + cryptographically verifies a bearer grant token, returning
    /// the authenticated principal and the verified grant. A revoked bearer
    /// is rejected here — exactly as dead as a bad signature.
    ///
    /// # Errors
    ///
    /// Returns [`IjimaError::InvalidInput`] on a malformed,
    /// bad-signature, or **revoked** token.
    pub fn verify_bearer(&self, bearer: &str) -> Result<AuthenticatedPrincipal> {
        if self.is_revoked(bearer) {
            return Err(IjimaError::invalid_input("token revoked"));
        }
        let buf = B64
            .decode(bearer.trim())
            .map_err(|e| IjimaError::invalid_input(format!("base64 decode: {e}")))?;
        let grant = GrantToken::from_bytes(&buf)
            .map_err(|e| IjimaError::invalid_input(format!("grant decode: {e}")))?;
        self.grant_verifier
            .verify(&grant)
            .map_err(|e| IjimaError::invalid_input(format!("grant verify: {e}")))?;
        Ok(AuthenticatedPrincipal {
            principal: grant.principal.clone(),
            grant,
            controller: Arc::clone(&self.controller),
            grant_verifier: Arc::clone(&self.grant_verifier),
        })
    }

    /// Convenience guard for handlers: verifies the token (authn) and
    /// authorizes via geometric containment — succeeds when the grant
    /// implies `required` (see [`AuthenticatedPrincipal::may`]).
    ///
    /// # Errors
    ///
    /// Returns an error if the token is invalid or does not imply `required`.
    pub fn require(&self, bearer: &str, required: &str) -> Result<AuthenticatedPrincipal> {
        let principal = self.verify_bearer(bearer)?;
        if principal.may(required) {
            Ok(principal)
        } else {
            Err(IjimaError::invalid_input(format!(
                "access denied: grant does not imply '{required}'"
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ijima_core::capabilities::{ADMIN, KNOWLEDGE_READ, MEMORY_READ, MEMORY_WRITE};

    fn fresh() -> IjimaAuth {
        IjimaAuth::from_embedded_policy().expect("embedded policy must load")
    }

    #[test]
    fn embedded_policy_loads_on_gr_4_8() {
        let auth = fresh();
        assert_eq!(auth.grassmannian(), (4, 8));
    }

    #[test]
    fn issue_then_verify_round_trips() {
        let auth = fresh();
        let bearer = auth
            .issue_bearer("elliott", MEMORY_READ)
            .expect("must issue");
        let principal = auth.verify_bearer(&bearer).expect("must verify");
        assert_eq!(principal.principal.as_str(), "elliott");
        assert_eq!(
            principal.granted_capabilities(),
            vec![MEMORY_READ.to_string()]
        );
    }

    #[test]
    fn tampered_signature_is_rejected() {
        let auth = fresh();
        let mut buf = B64
            .decode(
                auth.issue_bearer("elliott", MEMORY_READ)
                    .expect("must issue"),
            )
            .unwrap();
        // Flip the last byte (part of the 64-byte signature).
        let last = buf.len() - 1;
        buf[last] ^= 0xff;
        let tampered = B64.encode(&buf);
        assert!(auth.verify_bearer(&tampered).is_err());
    }

    #[test]
    fn admin_grant_implies_any_capability_via_geometry() {
        // No string short-circuit: admin [4,4,4,4] ≥ every partition.
        let auth = fresh();
        let bearer = auth.issue_bearer("root", ADMIN).expect("must issue");
        let principal = auth.require(&bearer, MEMORY_READ).expect("admin may read");
        assert_eq!(principal.principal.as_str(), "root");
        // And write, knowledge:read — all implied by the point class.
        assert!(auth.require(&bearer, MEMORY_WRITE).is_ok());
        assert!(auth.require(&bearer, KNOWLEDGE_READ).is_ok());
    }

    #[test]
    fn read_does_not_imply_write() {
        // [1] ≱ [2]: a read grant must not satisfy a write requirement.
        let auth = fresh();
        let bearer = auth.issue_bearer("alice", MEMORY_READ).expect("must issue");
        assert!(auth.require(&bearer, MEMORY_WRITE).is_err());
    }

    #[test]
    fn write_implies_read() {
        // The geometric upgrade: [2] ≥ [1], so memory:write grants
        // memory:read. This is the safe least-privilege direction.
        let auth = fresh();
        let bearer = auth.issue_bearer("bob", MEMORY_WRITE).expect("must issue");
        assert!(auth.require(&bearer, MEMORY_READ).is_ok());
    }

    #[test]
    fn unknown_required_capability_is_denied() {
        let auth = fresh();
        let bearer = auth.issue_bearer("alice", MEMORY_READ).expect("must issue");
        let principal = auth.verify_bearer(&bearer).expect("must verify");
        assert!(!principal.may("memory:nonexistent"));
    }

    #[test]
    fn multi_capability_grant() {
        // One token, several capabilities — the pi-shim 1-token model.
        let auth = fresh();
        let bearer = auth
            .issue_grant_bearer("pi", &[MEMORY_READ, MEMORY_WRITE, KNOWLEDGE_READ])
            .expect("must issue");
        let principal = auth.verify_bearer(&bearer).expect("must verify");
        assert_eq!(principal.principal.as_str(), "pi");
        // All three implied (write also implies read, redundantly).
        assert!(principal.may(MEMORY_READ));
        assert!(principal.may(MEMORY_WRITE));
        assert!(principal.may(KNOWLEDGE_READ));
    }

    #[test]
    fn empty_grant_rejected() {
        let auth = fresh();
        assert!(auth.issue_grant_bearer("x", &[]).is_err());
    }

    #[test]
    fn unknown_capability_rejected_at_issue() {
        let auth = fresh();
        assert!(auth.issue_bearer("x", "bogus:cap").is_err());
    }

    #[test]
    fn malformed_bearer_rejected() {
        let auth = fresh();
        assert!(auth.verify_bearer("not-base64!!!").is_err());
        assert!(auth.verify_bearer("").is_err());
    }

    #[test]
    fn seed_based_issue_then_verify_across_instances() {
        // CLI→daemon flow: one IjimaAuth (CLI) issues with a seed; a second
        // (daemon) from the SAME seed verifies the grant.
        let seed = IjimaAuth::generate_seed();
        let issuer = IjimaAuth::from_embedded_policy_with_seed(seed).expect("issuer");
        let bearer = issuer
            .issue_grant_bearer("elliott", &[MEMORY_READ, MEMORY_WRITE])
            .expect("must issue");
        let public_key = issuer.issuer_public_key_hex();
        assert_eq!(public_key.len(), 64);

        let daemon = IjimaAuth::from_embedded_policy_with_seed(seed).expect("daemon");
        let principal = daemon.verify_bearer(&bearer).expect("must verify");
        assert_eq!(principal.principal.as_str(), "elliott");
        assert_eq!(daemon.issuer_public_key_hex(), public_key);
    }

    // ---------- revocation (WS1b) ----------

    #[test]
    fn revoked_bearer_is_rejected_after_crypto_verify_passes() {
        let auth = fresh();
        let bearer = auth
            .issue_bearer("elliott", MEMORY_READ)
            .expect("must issue");
        assert!(auth.verify_bearer(&bearer).is_ok()); // before: fine
        auth.revoke(&bearer_hash(&bearer));
        let err = auth.verify_bearer(&bearer).expect_err("must reject");
        assert!(err.to_string().contains("revoked"));
    }

    #[test]
    fn hydration_replaces_prior_set() {
        let auth = fresh();
        let b1 = auth.issue_bearer("a", MEMORY_READ).expect("issue");
        let b2 = auth.issue_bearer("b", MEMORY_READ).expect("issue");
        auth.revoke(&bearer_hash(&b1));
        assert!(auth.is_revoked(&b1));
        // Hydrate with only b2's revocation → b1 is live again (the store
        // is the source of truth, not the union).
        auth.hydrate_revocations(&[TokenRevocation {
            token_hash: bearer_hash(&b2),
            revoked_at_unix: 0,
            reason: None,
        }]);
        assert!(!auth.is_revoked(&b1));
        assert!(auth.is_revoked(&b2));
    }

    #[test]
    fn bearer_hash_is_sha256_hex_of_trimmed_bearer() {
        let h1 = bearer_hash("  abc  ");
        let h2 = bearer_hash("abc");
        let h3 = bearer_hash("Bearer abc");
        assert_eq!(h1, h2); // trimmed
        assert_eq!(h2, h3); // scheme prefix tolerated
        assert_eq!(h1.len(), 64); // sha256 hex
        assert!(h1.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
