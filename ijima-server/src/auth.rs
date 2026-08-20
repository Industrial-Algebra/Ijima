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
    crypto::{CapabilityIssuer, GrantPolicy, GrantToken, GrantVerifier, KeyStore},
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
        let entries = self.capability_entries(capabilities)?;
        let grant = self
            .issuer
            .issue_grant(principal, &entries)
            .map_err(|e| IjimaError::invalid_input(format!("grant issue: {e}")))?;
        Ok(B64.encode(GrantToken::to_bytes(&grant)))
    }

    /// Issues a grant that dies at `expires_at_unix` (Unix seconds,
    /// Schubert 0.5 ADR-0001: the boundary is inclusive — the grant is
    /// dead the instant `now >= expires_at`). Expiry is covered by the
    /// signature and enforced by [`GrantVerifier::verify`] standalone;
    /// expired bearers fail `verify_bearer` with an `expired` detail.
    ///
    /// # Errors
    ///
    /// Returns [`IjimaError::InvalidInput`] on unknown capabilities or
    /// issuer rejection — same contract as
    /// [`issue_grant_bearer`](Self::issue_grant_bearer).
    pub fn issue_grant_bearer_with_expiry(
        &self,
        principal: impl Into<PrincipalId>,
        capabilities: &[&str],
        expires_at_unix: u64,
    ) -> Result<String> {
        if capabilities.is_empty() {
            return Err(IjimaError::invalid_input(
                "grant must carry at least one capability",
            ));
        }
        let entries = self.capability_entries(capabilities)?;
        let grant = self
            .issuer
            .issue_grant_with_expiry(principal, &entries, expires_at_unix)
            .map_err(|e| IjimaError::invalid_input(format!("grant issue: {e}")))?;
        Ok(B64.encode(GrantToken::to_bytes(&grant)))
    }

    /// Policy-constrained issuance (Schubert 0.5 #20.3): signs only what
    /// `policy` entitles this principal to carry. Fails closed — an
    /// unknown principal or a capability outside the entitlement denies
    /// with [`schubert::SchubertError::GrantDeniedByPolicy`] detail (no
    /// smuggling a stronger geometry under an allowed id). `expires_at`
    /// passes through to the issuer (`None` = never, pre-0.5 behavior).
    ///
    /// This is the seam `ijima token issue` builds on; the unconstrained
    /// [`issue_grant_bearer`](Self::issue_grant_bearer) remains for test
    /// tooling and trusted offline flows.
    ///
    /// # Errors
    ///
    /// Returns [`IjimaError::InvalidInput`] when the policy denies the
    /// request, or for unknown capabilities.
    pub fn issue_grant_bearer_under_policy(
        &self,
        principal: impl Into<PrincipalId>,
        capabilities: &[&str],
        policy: &GrantPolicy,
        expires_at: Option<u64>,
    ) -> Result<String> {
        if capabilities.is_empty() {
            return Err(IjimaError::invalid_input(
                "grant must carry at least one capability",
            ));
        }
        let entries = self.capability_entries(capabilities)?;
        let principal = principal.into();
        policy
            .may_issue(&principal, &entries)
            .map_err(|e| IjimaError::invalid_input(format!("grant denied by policy: {e}")))?;
        let grant = match expires_at {
            Some(at) => self.issuer.issue_grant_with_expiry(principal, &entries, at),
            None => self.issuer.issue_grant(principal, &entries),
        }
        .map_err(|e| IjimaError::invalid_input(format!("grant issue: {e}")))?;
        Ok(B64.encode(GrantToken::to_bytes(&grant)))
    }

    /// Resolves capability names to signed `(CapabilityId, partition)`
    /// pairs via the loaded policy's partition map.
    ///
    /// # Errors
    ///
    /// Returns [`IjimaError::InvalidInput`] for any name not in the
    /// policy vocabulary.
    fn capability_entries(&self, capabilities: &[&str]) -> Result<Vec<(CapabilityId, Vec<usize>)>> {
        let mut entries = Vec::with_capacity(capabilities.len());
        for cap in capabilities {
            let partition = self
                .controller
                .capability(cap)
                .map(|c| c.partition.clone())
                .ok_or_else(|| IjimaError::invalid_input(format!("unknown capability: {cap}")))?;
            entries.push((CapabilityId::new(*cap), partition));
        }
        Ok(entries)
    }

    /// The grant verifier (exposes `verify_at` for clock-injected checks).
    pub fn grant_verifier(&self) -> &GrantVerifier {
        &self.grant_verifier
    }

    /// Resolves the issuance policy for `ijima token issue` (Schubert 0.5
    /// #20.3): an explicit `--policy` path wins (unreadable = hard error
    /// — an explicit pointer must be honored); then `$IJIMA_POLICY`
    /// (same hard-error rule); then `$IJIMA_DIR/policy.toml` if present;
    /// otherwise the embedded default (which seeds no principals — a
    /// fresh install mints nothing until the operator provisions a
    /// policy file).
    ///
    /// # Errors
    ///
    /// Returns [`IjimaError::InvalidInput`] when an explicit/env policy
    /// path cannot be read or the fallback resolution fails.
    pub fn resolve_issuance_policy(explicit: Option<&std::path::Path>) -> Result<String> {
        let env_path = std::env::var_os("IJIMA_POLICY").map(std::path::PathBuf::from);
        let dir = std::env::var_os("IJIMA_DIR").map(std::path::PathBuf::from);
        Ok(
            resolve_policy_source(explicit, env_path.as_deref(), dir.as_deref())?
                .unwrap_or_else(|| POLICY_TOML.to_string()),
        )
    }

    /// Builds the [`schubert::policy::PolicyConfig`] that constrains issuance
    /// from a resolved policy source. Two shapes are accepted:
    ///
    /// - **Full policy** (contains `[capabilities]`): parsed and validated
    ///   as a complete policy. Must match the embedded partition map the
    ///   daemon verifies with — a diverging geometry is an operator error,
    ///   surfaced as a hard parse error.
    /// - **Principals-only overlay** (the operator-friendly default):
    ///   `[principals.<name>] grants = [...]` sections merged onto the
    ///   embedded policy. Partitions always derive from the embedded
    ///   policy, so an overlay can only *assign* existing capabilities —
    ///   never redefine the geometry (the #20.3 anti-smuggling invariant).
    ///   The overlay's principal map is authoritative: removing a principal
    ///   removes their issuance entitlement (already-issued grants keep
    ///   verifying — they are proof-carrying — until expiry or revocation).
    ///
    /// # Errors
    ///
    /// Returns [`IjimaError::InvalidInput`] when neither shape parses, an
    /// overlay carries non-principal sections, or the merged config fails
    /// validation.
    pub fn issuance_policy_from_source(toml_str: &str) -> Result<schubert::policy::PolicyConfig> {
        #[derive(serde::Deserialize)]
        struct PrincipalsOverlay {
            #[serde(default)]
            principals: std::collections::BTreeMap<String, schubert::policy::PrincipalConfig>,
        }

        if toml_str.contains("[capabilities") {
            let cfg = schubert::policy::PolicyConfig::from_toml(toml_str)
                .map_err(|e| IjimaError::invalid_input(format!("policy parse: {e}")))?;
            cfg.validate()
                .map_err(|e| IjimaError::invalid_input(format!("policy validate: {e}")))?;
            return Ok(cfg);
        }

        // Overlay: principals only, on top of the embedded partitions.
        let raw: toml::Value = toml::from_str(toml_str)
            .map_err(|e| IjimaError::invalid_input(format!("policy overlay parse: {e}")))?;
        // Reject documents that try to do more than assign principals: any
        // top-level section other than `principals` is out of contract.
        if let Some(table) = raw.as_table() {
            for key in table.keys() {
                if key != "principals" {
                    return Err(IjimaError::invalid_input(format!(
                        "policy overlay may only contain [principals.*] (found `{key}`); \
                         a full policy must carry [capabilities] and validate as a whole"
                    )));
                }
            }
        }
        let overlay: PrincipalsOverlay = raw
            .try_into()
            .map_err(|e| IjimaError::invalid_input(format!("policy overlay parse: {e}")))?;
        if overlay.principals.is_empty() {
            return Err(IjimaError::invalid_input(
                "policy overlay declares no principals",
            ));
        }
        let mut merged = schubert::policy::PolicyConfig::from_toml(POLICY_TOML)
            .map_err(|e| IjimaError::invalid_input(format!("embedded policy: {e}")))?;
        merged.principals = overlay.principals;
        merged
            .validate()
            .map_err(|e| IjimaError::invalid_input(format!("policy validate: {e}")))?;
        Ok(merged)
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

/// Pure policy-source resolution core for
/// [`IjimaAuth::resolve_issuance_policy`]: explicit path (unreadable =
/// hard error) → env path (same rule) → `<dir>/policy.toml` if present →
/// `None` (caller falls back to the embedded default). Env-free so the
/// precedence chain is testable without env mutation.
///
/// # Errors
///
/// Returns [`IjimaError::InvalidInput`] when an explicit or env policy
/// path exists but cannot be read.
fn resolve_policy_source(
    explicit: Option<&std::path::Path>,
    env_path: Option<&std::path::Path>,
    dir: Option<&std::path::Path>,
) -> Result<Option<String>> {
    let read_or_err = |p: &std::path::Path, origin: &str| {
        std::fs::read_to_string(p).map(Some).map_err(|e| {
            IjimaError::invalid_input(format!("policy file {origin} {}: {e}", p.display()))
        })
    };
    if let Some(p) = explicit {
        return read_or_err(p, "(--policy)");
    }
    if let Some(p) = env_path {
        return read_or_err(p, "($IJIMA_POLICY)");
    }
    if let Some(dir) = dir {
        let candidate = dir.join("policy.toml");
        if candidate.exists() {
            return read_or_err(&candidate, "($IJIMA_DIR/policy.toml)");
        }
    }
    Ok(None)
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

    // ---- Schubert 0.5: expiry ----

    #[test]
    fn expired_grant_is_rejected_with_expired_detail() {
        let auth = fresh();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // Issued with an expiry in the past — dead on arrival.
        let bearer = auth
            .issue_grant_bearer_with_expiry("elliott", &[MEMORY_READ], now - 1)
            .expect("issue");
        let err = auth.verify_bearer(&bearer).expect_err("must be dead");
        let msg = err.to_string();
        assert!(msg.contains("expired"), "detail should name expiry: {msg}");
    }

    #[test]
    fn expiry_boundary_is_inclusive_at_verify_at() {
        let auth = fresh();
        let bearer = auth
            .issue_grant_bearer_with_expiry("elliott", &[MEMORY_READ], 1_000_000)
            .expect("issue");
        let buf = B64.decode(bearer.trim()).expect("b64");
        let grant = GrantToken::from_bytes(&buf).expect("grant");
        // ADR-0001: dead the instant now >= expires_at.
        assert!(auth.grant_verifier().verify_at(&grant, 999_999).is_ok());
        let err = auth
            .grant_verifier()
            .verify_at(&grant, 1_000_000)
            .expect_err("boundary is inclusive");
        assert!(matches!(err, schubert::SchubertError::GrantExpired { .. }));
    }

    #[test]
    fn unexpired_grant_with_expiry_still_verifies() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let auth = fresh();
        let bearer = auth
            .issue_grant_bearer_with_expiry("elliott", &[MEMORY_READ, MEMORY_WRITE], now + 3600)
            .expect("issue");
        let principal = auth.verify_bearer(&bearer).expect("valid for an hour");
        assert!(principal.may(MEMORY_WRITE));
    }

    // ---- Schubert 0.5: policy-constrained issuance ----

    fn policy_with_alice() -> schubert::policy::PolicyConfig {
        let toml = format!(
            "{POLICY_TOML}\n[principals.alice]\ngrants = [\"memory:read\", \"memory:write\"]\n"
        );
        schubert::policy::PolicyConfig::from_toml(&toml).expect("policy parses")
    }

    #[test]
    fn under_policy_allows_entitled_request() {
        let auth = fresh();
        let policy =
            schubert::crypto::GrantPolicy::from_policy(&policy_with_alice()).expect("grant policy");
        let bearer = auth
            .issue_grant_bearer_under_policy("alice", &[MEMORY_READ], &policy, None)
            .expect("alice is entitled to memory:read");
        let principal = auth.verify_bearer(&bearer).expect("verify");
        assert_eq!(principal.principal.as_str(), "alice");
    }

    #[test]
    fn under_policy_denies_unknown_principal() {
        let auth = fresh();
        let policy =
            schubert::crypto::GrantPolicy::from_policy(&policy_with_alice()).expect("grant policy");
        let err = auth
            .issue_grant_bearer_under_policy("mallory", &[MEMORY_READ], &policy, None)
            .expect_err("fails closed on unknown principals");
        assert!(err.to_string().contains("mallory"));
    }

    #[test]
    fn under_policy_denies_over_entitled_request() {
        let auth = fresh();
        let policy =
            schubert::crypto::GrantPolicy::from_policy(&policy_with_alice()).expect("grant policy");
        let err = auth
            .issue_grant_bearer_under_policy("alice", &[ADMIN], &policy, None)
            .expect_err("alice cannot smuggle admin");
        assert!(err.to_string().contains("admin"));
    }

    #[test]
    fn principals_only_overlay_merges_onto_embedded_partitions() {
        let overlay = "[principals.elliott]\ngrants = [\"memory:read\", \"memory:write\"]\n";
        let cfg = IjimaAuth::issuance_policy_from_source(overlay).expect("overlay");
        // Partitions come from the embedded policy (anti-smuggling).
        let read = cfg.capabilities.get(MEMORY_READ).expect("embedded cap");
        assert!(!read.partition.is_empty());
        // The overlay principal is entitled.
        let grants = cfg.grants_for("elliott");
        assert_eq!(grants.len(), 2);
        assert!(cfg.grants_for("mallory").is_empty());
    }

    #[test]
    fn overlay_rejects_non_principal_sections() {
        let bad = "[principals.elliott]\ngrants = [\"memory:read\"]\n\n[grassmannian]\nk = 9\n";
        let err = IjimaAuth::issuance_policy_from_source(bad)
            .expect_err("overlay may not touch geometry");
        assert!(err.to_string().contains("grassmannian"));
    }

    #[test]
    fn overlay_with_no_principals_is_rejected() {
        let err = IjimaAuth::issuance_policy_from_source("# nothing\n").expect_err("empty overlay");
        assert!(err.to_string().contains("no principals"));
    }

    #[test]
    fn policy_source_precedence_explicit_env_dir_fallback() {
        let tmp = std::env::temp_dir().join(format!("ijima-pol-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).expect("mkdir");
        let explicit = tmp.join("explicit.toml");
        let env = tmp.join("env.toml");
        let dir_policy = tmp.join("policy.toml");
        std::fs::write(&explicit, "# explicit").expect("w");
        std::fs::write(&env, "# env").expect("w");
        std::fs::write(&dir_policy, "# dir").expect("w");

        // explicit wins over env and dir
        assert_eq!(
            resolve_policy_source(Some(&explicit), Some(&env), Some(&tmp))
                .expect("res")
                .as_deref(),
            Some("# explicit")
        );
        // env wins over dir
        assert_eq!(
            resolve_policy_source(None, Some(&env), Some(&tmp))
                .expect("res")
                .as_deref(),
            Some("# env")
        );
        // dir/policy.toml picked up when present
        assert_eq!(
            resolve_policy_source(None, None, Some(&tmp))
                .expect("res")
                .as_deref(),
            Some("# dir")
        );
        // no dir policy → None (caller falls back to embedded)
        let empty = tmp.join("empty");
        std::fs::create_dir_all(&empty).expect("mkdir");
        assert_eq!(
            resolve_policy_source(None, None, Some(&empty)).expect("res"),
            None
        );
        // explicit pointer at a missing file = hard error
        let missing = tmp.join("missing.toml");
        assert!(resolve_policy_source(Some(&missing), None, None).is_err());
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
