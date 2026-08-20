// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! Memory namespaces — the isolation boundary for multi-user access (D2).
//!
//! Every read and write through the [`crate::Store`] is scoped to a
//! [`NamespaceId`]. A namespace is the unit of visibility: an operator's
//! private memory, a shared project namespace, or the global commons
//! (the legacy pi-mempalace "everyone sees everything" mode).

/// Identifies a memory namespace.
///
/// Wire form is a stable opaque string (e.g. `ns_elliott_private`,
/// `ns_ijima_shared`, `ns_global`). v0 stores this as a SurrealDB record
/// field; a future promotion maps each namespace onto a native SurrealDB
/// `NS`/`DB` scope for first-class isolation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct NamespaceId(pub String);

impl NamespaceId {
    /// Construct a namespace id from any string-like value.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The stable wire string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The visibility/isolation class of a namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum NamespaceKind {
    /// One operator's personal memory — visible only to that operator.
    Private,
    /// A project/team namespace — visible to a configured group.
    Shared,
    /// The global commons — visible to everyone (legacy pi-mempalace mode).
    Global,
}

/// A namespace descriptor: its id, visibility class, and owning operator.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Namespace {
    /// Stable opaque identifier.
    pub id: NamespaceId,
    /// Visibility class.
    pub kind: NamespaceKind,
    /// Owning operator principal id (`"system"` for the global commons).
    pub owner: String,
}

/// The global doctrine namespace — curated, Git-versioned memories
/// ([`crate::MemorySource::Doctrine`]) mirrored from the seed pack.
/// Readable by every principal via `?namespace=ns_doctrine`.
pub const DOCTRINE_NAMESPACE: &str = "ns_doctrine";

/// A principal's membership in a shared namespace — the WS3 org-wall
/// grant. Membership is store-backed (mutable at runtime, no redeploy);
/// see the namespace-membership ADR.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NamespaceMembership {
    /// The shared namespace (`ns_<org>_shared`).
    pub namespace: String,
    /// The principal granted access.
    pub principal: String,
    /// Unix seconds when the grant was recorded.
    pub granted_at_unix: u64,
    /// The admin principal who issued the grant (audit trail).
    pub granted_by: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_id_round_trips() {
        let ns = NamespaceId::new("ns_elliott_private");
        assert_eq!(ns.as_str(), "ns_elliott_private");
        assert_eq!(ns, NamespaceId("ns_elliott_private".into()));
    }

    #[test]
    fn global_namespace_owner_is_system() {
        let global = Namespace {
            id: NamespaceId::new("ns_global"),
            kind: NamespaceKind::Global,
            owner: "system".into(),
        };
        assert_eq!(global.kind, NamespaceKind::Global);
        assert_eq!(global.owner, "system");
    }
}
