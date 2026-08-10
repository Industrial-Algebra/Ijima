// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! Authentication + authorization via Schubert proof-carrying capability
//! tokens.
//!
//! Per `docs/DESIGN.md` D4, Ijima uses Schubert for **both** authn and
//! authz: a [`schubert::crypto::CapabilityToken`] is Ed25519-signed by an
//! issuer and carries a `principal` + `capability`. Verifying the
//! signature authenticates the principal; the [`AccessController`]
//! authorizes the action geometrically on Gr(4,8).
//!
//! The capability vocabulary is declarative TOML at
//! [`policy/policy.toml`](https://github.com/Industrial-Algebra/Ijima) —
//! selected by Schubert's `recommend` CLI for Ijima's constraints
//! (Gr(4,8), dim 16, features std/crypto/policy).
//!
//! ## Wire format
//!
//! Bearer tokens are base64 of a length-prefixed binary blob:
//!
//! ```text
//! u16 BE principal_len | principal utf-8
//! u16 BE capability_len | capability utf-8
//! 32 bytes issuer public key
//! 64 bytes Ed25519 signature
//! ```
//!
//! The signature covers `principal \\0 capability \\0 issuer_key` (the
//! exact message Schubert's issuer signs), so the wire format is pure
//! transport — verification re-checks the signature cryptographically.

use base64::{Engine, engine::general_purpose::STANDARD as B64};
use ijima_core::{IjimaError, Result};
use schubert::{
    AccessController, AccessDecision, PrincipalId,
    crypto::{CapabilityIssuer, CapabilityToken, CapabilityVerifier},
};

/// Ijima's Schubert policy, embedded at compile time.
const POLICY_TOML: &str = include_str!("../../policy/policy.toml");

const ISSUER_KEY_LEN: usize = 32;
const SIGNATURE_LEN: usize = 64;

/// The authenticated principal + capability carried by a verified token.
///
/// Produced by [`IjimaAuth::verify_bearer`]. Handlers consult the
/// `capability` field via [`may`](Self::may) to enforce a specific
/// capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedPrincipal {
    /// The principal this token was issued to.
    pub principal: PrincipalId,
    /// The single capability this token grants.
    pub capability: String,
}

impl AuthenticatedPrincipal {
    /// Returns true if this token's capability grants `required`,
    /// directly or via the `admin` (point-class) capability.
    pub fn may(&self, required: &str) -> bool {
        self.capability == required || self.capability == ijima_core::capabilities::ADMIN
    }

    /// This principal's default personal namespace id
    /// (`ns_<principal>_private`). Every request is scoped to this
    /// namespace unless explicit namespace parameters land later.
    pub fn personal_namespace(&self) -> ijima_core::NamespaceId {
        ijima_core::NamespaceId::new(format!("ns_{}_private", self.principal.as_str()))
    }
}

/// Ijima's auth core: an [`AccessController`] (authz) plus a capability
/// token issuer and verifier (authn) sharing one Ed25519 key.
///
/// A daemon constructs one of these at startup; an admin CLI uses the
/// issuer to mint tokens via [`IjimaAuth::issue_bearer`].
#[derive(Debug)]
pub struct IjimaAuth {
    controller: AccessController,
    issuer: CapabilityIssuer,
    verifier: CapabilityVerifier,
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
    /// 32-byte Ed25519 seed. The same seed must be shared by every
    /// process that issues or verifies tokens for this Ijima instance.
    ///
    /// # Errors
    ///
    /// Returns [`IjimaError::InvalidInput`] if the policy TOML is invalid.
    pub fn from_embedded_policy_with_seed(seed: [u8; 32]) -> Result<Self> {
        let controller = AccessController::from_policy_toml(POLICY_TOML)
            .map_err(|e| IjimaError::invalid_input(format!("policy load: {e}")))?;
        let issuer = CapabilityIssuer::from_seed(seed);
        let verifier = CapabilityVerifier::new(issuer.public_key());
        Ok(Self {
            controller,
            issuer,
            verifier,
        })
    }

    /// Generates a fresh random 32-byte issuer seed (for first-time setup).
    pub fn generate_seed() -> [u8; 32] {
        use rand::TryRngCore;
        let mut seed = [0u8; 32];
        rand::rngs::OsRng
            .try_fill_bytes(&mut seed)
            .expect("OsRng is infallible in practice");
        seed
    }

    /// The issuer's Ed25519 public key as lowercase hex, for distribution
    /// to verifiers and operator visibility.
    pub fn issuer_public_key_hex(&self) -> String {
        self.issuer
            .public_key()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    /// Returns the Grassmannian the controller operates on.
    pub fn grassmannian(&self) -> (usize, usize) {
        self.controller.grassmannian()
    }

    /// Issues a bearer token (base64 wire format) granting `capability`
    /// to `principal`.
    ///
    /// # Errors
    ///
    /// Returns [`IjimaError::InvalidInput`] if Schubert's issuer rejects
    /// the inputs.
    pub fn issue_bearer(
        &self,
        principal: impl Into<PrincipalId>,
        capability: impl AsRef<str>,
    ) -> Result<String> {
        let capability_str = capability.as_ref();
        let token = self
            .issuer
            .issue(principal, capability_str)
            .map_err(|e| IjimaError::invalid_input(format!("token issue: {e}")))?;
        encode_token(&token)
    }

    /// Decodes + cryptographically verifies a bearer token, returning the
    /// authenticated principal and the capability the token grants.
    ///
    /// # Errors
    ///
    /// Returns [`IjimaError::InvalidInput`] on a malformed or
    /// bad-signature token.
    pub fn verify_bearer(&self, bearer: &str) -> Result<AuthenticatedPrincipal> {
        let token = decode_token(bearer)?;
        let (principal, capability) = self
            .verifier
            .verify_and_extract(&token)
            .map_err(|e| IjimaError::invalid_input(format!("token verify: {e}")))?;
        Ok(AuthenticatedPrincipal {
            principal: principal.clone(),
            capability: capability.as_str().to_string(),
        })
    }

    /// Authorizes `principal` for `required` capabilities via the
    /// geometric Schubert check. Returns the [`AccessDecision`].
    ///
    /// This is the authorization half; combine with [`verify_bearer`] in
    /// handlers that need both ("who is calling" + "may they do this").
    pub fn check(&self, principal: &PrincipalId, required: &[&str]) -> Result<AccessDecision> {
        self.controller
            .check(principal, required)
            .map_err(|e| IjimaError::invalid_input(format!("access check: {e}")))
    }

    /// Convenience guard for handlers: verifies the token (authn) and
    /// authorizes via **proof-carrying** semantics — the token's capability
    /// field is the authorization, verified cryptographically against the
    /// issuer's public key. Succeeds when the token grants exactly
    /// `required` or grants [`ADMIN`](ijima_core::capabilities::ADMIN)
    /// (the point class implies every capability).
    ///
    /// For richer geometric implication (e.g. "mining:trigger composes over
    /// session:ingest"), use [`check`](Self::check) against a controller
    /// with explicit grants — that path is for offline policy analysis,
    /// not per-request runtime checks.
    ///
    /// # Errors
    ///
    /// Returns an error if the token is invalid or does not grant `required`.
    pub fn require(&self, bearer: &str, required: &str) -> Result<AuthenticatedPrincipal> {
        let principal = self.verify_bearer(bearer)?;
        if principal.capability == required
            || principal.capability == ijima_core::capabilities::ADMIN
        {
            Ok(principal)
        } else {
            Err(IjimaError::invalid_input(format!(
                "access denied: token grants '{}' but '{}' is required",
                principal.capability, required
            )))
        }
    }
}

// ---------- wire encoding ----------

fn encode_token(token: &CapabilityToken) -> Result<String> {
    let p = token.principal.as_str().as_bytes();
    let c = token.capability.as_str().as_bytes();
    if p.len() > u16::MAX as usize || c.len() > u16::MAX as usize {
        return Err(IjimaError::invalid_input("token field too long"));
    }
    let mut buf = Vec::with_capacity(2 + p.len() + 2 + c.len() + ISSUER_KEY_LEN + SIGNATURE_LEN);
    buf.extend_from_slice(&(p.len() as u16).to_be_bytes());
    buf.extend_from_slice(p);
    buf.extend_from_slice(&(c.len() as u16).to_be_bytes());
    buf.extend_from_slice(c);
    if token.issuer_key.len() != ISSUER_KEY_LEN || token.signature.len() != SIGNATURE_LEN {
        return Err(IjimaError::invalid_input(
            "malformed issuer key or signature",
        ));
    }
    buf.extend_from_slice(&token.issuer_key);
    buf.extend_from_slice(&token.signature);
    Ok(B64.encode(&buf))
}

fn decode_token(bearer: &str) -> Result<CapabilityToken> {
    let buf = B64
        .decode(bearer.trim())
        .map_err(|e| IjimaError::invalid_input(format!("base64 decode: {e}")))?;
    let mut pos = 0;
    let plen = read_u16(&buf, &mut pos)?;
    let principal = read_str(&buf, &mut pos, plen)?;
    let clen = read_u16(&buf, &mut pos)?;
    let capability = read_str(&buf, &mut pos, clen)?;
    let issuer_key = read_bytes(&buf, &mut pos, ISSUER_KEY_LEN)?;
    let signature = read_bytes(&buf, &mut pos, SIGNATURE_LEN)?;
    if pos != buf.len() {
        return Err(IjimaError::invalid_input("trailing bytes in token"));
    }
    Ok(CapabilityToken {
        principal: PrincipalId::new(principal),
        capability: schubert::CapabilityId::new(capability),
        issuer_key: issuer_key.to_vec(),
        signature: signature.to_vec(),
    })
}

fn read_u16(buf: &[u8], pos: &mut usize) -> Result<usize> {
    if *pos + 2 > buf.len() {
        return Err(IjimaError::invalid_input("truncated token length"));
    }
    let v = u16::from_be_bytes([buf[*pos], buf[*pos + 1]]) as usize;
    *pos += 2;
    Ok(v)
}

fn read_str(buf: &[u8], pos: &mut usize, len: usize) -> Result<String> {
    let bytes = read_bytes(buf, pos, len)?;
    String::from_utf8(bytes.to_vec())
        .map_err(|e| IjimaError::invalid_input(format!("non-utf8 token field: {e}")))
}

fn read_bytes<'a>(buf: &'a [u8], pos: &mut usize, len: usize) -> Result<&'a [u8]> {
    if *pos + len > buf.len() {
        return Err(IjimaError::invalid_input("truncated token field"));
    }
    let slice = &buf[*pos..*pos + len];
    *pos += len;
    Ok(slice)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ijima_core::capabilities::{ADMIN, MEMORY_READ, MEMORY_WRITE};

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
        assert_eq!(principal.capability, MEMORY_READ);
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
    fn admin_token_grants_any_capability() {
        // Proof-carrying semantics: an ADMIN token (point class) implies
        // every capability, so it satisfies a memory:read requirement.
        let auth = fresh();
        let bearer = auth.issue_bearer("root", ADMIN).expect("must issue");
        let principal = auth.require(&bearer, MEMORY_READ).expect("admin may read");
        assert_eq!(principal.principal.as_str(), "root");
    }

    #[test]
    fn read_token_does_not_grant_write() {
        let auth = fresh();
        let bearer = auth.issue_bearer("alice", MEMORY_READ).expect("must issue");
        // A principal only has the capability in their token here (the
        // controller has no grants yet), so requiring MEMORY_WRITE must
        // fail. This pins the authz half.
        assert!(auth.require(&bearer, MEMORY_WRITE).is_err());
    }

    #[test]
    fn malformed_bearer_rejected() {
        let auth = fresh();
        assert!(auth.verify_bearer("not-base64!!!").is_err());
        assert!(auth.verify_bearer("").is_err());
    }

    #[test]
    fn seed_based_issue_then_verify_across_instances() {
        // Simulates the CLI→daemon flow: one IjimaAuth (the CLI) issues
        // with a seed; a second IjimaAuth (the daemon) constructed from
        // the SAME seed verifies the token.
        let seed = IjimaAuth::generate_seed();
        let issuer = IjimaAuth::from_embedded_policy_with_seed(seed).expect("issuer");
        let bearer = issuer
            .issue_bearer("elliott", MEMORY_READ)
            .expect("must issue");
        let public_key = issuer.issuer_public_key_hex();
        assert_eq!(public_key.len(), 64);

        // A *different* instance with the same seed must verify.
        let daemon = IjimaAuth::from_embedded_policy_with_seed(seed).expect("daemon");
        let principal = daemon.verify_bearer(&bearer).expect("must verify");
        assert_eq!(principal.principal.as_str(), "elliott");
        assert_eq!(principal.capability, MEMORY_READ);
        // And derive the same public key.
        assert_eq!(daemon.issuer_public_key_hex(), public_key);
    }
}
