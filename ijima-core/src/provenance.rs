// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! Provenance newtypes — the identity/authority stamp every [`Memory`]
//! carries.
//!
//! See `docs/adr/provenance-tier-model.md`. These are the substrate for
//! Phase 4 (context-poisoning source-tracing) and Phase 5 (federation
//! provenance + authority-scoped conflict resolution). For 0.1.0
//! (single instance) they default to the local instance; the fields are
//! present so the schema doesn't churn when federation lands.

/// The stable identifier of the Ijima instance that authored a record.
///
/// Typed newtype (IA convention) so an instance id is never confused with
/// a free string, a [`crate::NamespaceId`], or an
/// [`AuthorityScope`](self::AuthorityScope).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct InstanceId(pub String);

impl InstanceId {
    /// The local instance id — the author of every record in a
    /// single-instance 0.1.0 deployment.
    ///
    /// This is a pure constant (`"local"`) in the domain crate; the
    /// `IJIMA_INSTANCE_ID` override is a Phase 5 concern, applied at the
    /// server boundary once instances actually matter. Keeping core
    /// process-environment-free preserves the "pure, backend-free" contract.
    pub fn local() -> Self {
        Self("local".to_string())
    }
}

impl Default for InstanceId {
    fn default() -> Self {
        Self::local()
    }
}

/// The scope (instance + namespace/project) that is **source-of-truth** for
/// a record — the authority a receiver defers to on conflict (Phase 5).
///
/// For 0.1.0 it defaults to the local instance's scope; not exercised until
/// federation, but present so the schema is forward-compatible.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct AuthorityScope(pub String);

impl AuthorityScope {
    /// The local authority scope — the source-of-truth for a record in a
    /// single-instance 0.1.0 deployment.
    pub fn local() -> Self {
        Self(InstanceId::local().0)
    }
}

impl Default for AuthorityScope {
    fn default() -> Self {
        Self::local()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_instance_id_is_the_constant_local() {
        // Pure domain default — no env coupling (the IJIMA_INSTANCE_ID
        // override is a Phase 5 server-boundary concern).
        assert_eq!(InstanceId::local().0, "local");
        assert_eq!(InstanceId::default().0, "local");
    }

    #[test]
    fn local_authority_scope_mirrors_local_instance() {
        assert_eq!(AuthorityScope::local().0, "local");
        assert_eq!(AuthorityScope::default().0, InstanceId::local().0);
    }

    #[test]
    fn instance_id_is_type_distinct_from_authority_scope() {
        // Both wrap String but are distinct newtypes — cannot be
        // confused at the type level (IA newtype convention).
        let _: InstanceId = InstanceId("prod-hub".into());
        let _: AuthorityScope = AuthorityScope("prod-hub/ijima".into());
        assert_ne!(InstanceId("x".into()), InstanceId("y".into()));
    }
}
