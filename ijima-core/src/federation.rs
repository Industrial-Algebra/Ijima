// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! The federation control-API wire contract — Ijima's server-side DTOs for
//! the surface Dominic's [`FederationClient`](../../Dominic) consumes.
//!
//! This module is the canonical home for the federation wire types; dominic-core
//! currently carries a duplicate set (its topology/federation modules) which it
//! anticipates unifying here once it depends on `ijima-core` (see the doc
//! comment on dominic-core's `InstanceId`). The types mirror dominic-core's
//! shapes **exactly** so the JSON is byte-compatible without either crate
//! depending on the other — the wire format is the shared spec.
//!
//! Status: **scaffold** (ADR `docs/adr/federation-control-api.md`). The DTOs +
//! routes are in place; non-bypassable boundary enforcement (trust-tier egress
//! filtering, scope/airgap deny, boundary transformation) is deferred — see the
//! ADR's "Deferred" section. Today these routes apply writes locally with
//! provenance stamping but do not yet enforce the federation safety floor.
//!
//! (Federation design seed: `docs/discovery/networked-instances-federation.md`;
//! provenance foundation: `provenance.rs` + ADR `provenance-tier-model.md`.)

#![cfg(feature = "federation")]

use serde::{Deserialize, Serialize};

/// A stable instance identifier (`"local"` for a single-instance 0.1.0
/// deployment). Newtype — serde serializes it as its inner string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InstanceId(pub String);

impl InstanceId {
    /// Constructs an instance id.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The local instance id (single-instance default).
    #[must_use]
    pub fn local() -> Self {
        Self("local".to_string())
    }
}

impl Default for InstanceId {
    fn default() -> Self {
        Self::local()
    }
}

/// The role an Ijima instance plays in the federation topology (federation
/// seed §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InstanceRole {
    /// Aggregator / authoritative hub.
    Unifying,
    /// Cold storage / backup / larger-storage tier.
    Archive,
    /// Source-of-truth for a specific domain; others defer to it there.
    DomainAuthority,
    /// Offline-capable replica that syncs to a central instance.
    Edge,
    /// Sovereign; default-deny egress.
    Airgapped,
}

/// The scope (namespace/project) an instance is authoritative for. Drives
/// source-authority conflict resolution.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AuthoritativeScope {
    /// Namespace the instance is authoritative for.
    pub namespace: String,
    /// Project the instance is authoritative for.
    pub project: String,
}

impl AuthoritativeScope {
    /// Constructs an authoritative scope.
    #[must_use]
    pub fn new(namespace: impl Into<String>, project: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            project: project.into(),
        }
    }
}

/// Direction of memory flow on a federation link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LinkDirection {
    /// A → B, read-only.
    Replica,
    /// Bidirectional.
    Sync,
    /// B pulls.
    Subscribe,
    /// Hard deny (no flow).
    Airgap,
}

/// How write conflicts are resolved when two instances touch the same scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConflictResolution {
    /// The authoritative instance for the scope wins (default).
    SourceAuthority,
    /// Highest timestamp wins.
    LastWriteWins,
    /// CRDT-style merge.
    CrdtMerge,
}

/// How fresh cross-instance data must be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Freshness {
    /// Streaming.
    Realtime,
    /// Scheduled + offline queue.
    Batched,
}

/// A federation link policy between two instances (federation seed §4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkPolicy {
    /// Direction of memory flow.
    pub direction: LinkDirection,
    /// Conflict resolution strategy.
    pub conflict: ConflictResolution,
    /// Freshness requirement.
    pub freshness: Freshness,
}

/// An outbound link this instance declares to a peer (the wire form carried in
/// [`FederationState::outbound_links`]; the graph edge's `source` is the
/// instance itself).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboundLink {
    /// The peer instance the link points to.
    pub target: InstanceId,
    /// The link's policy.
    pub policy: LinkPolicy,
}

/// A routed-write operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WriteOperation {
    /// Create a new record.
    Create,
    /// Update an existing record.
    Update,
    /// Delete a record.
    Delete,
}

/// `GET /federation/state` response: an instance's federated view + a
/// reference to its capability policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FederationState {
    /// The instance this state describes.
    pub instance_id: InstanceId,
    /// The instance's federation role.
    pub role: InstanceRole,
    /// Scopes this instance is authoritative for.
    pub authoritative_scopes: Vec<AuthoritativeScope>,
    /// Outbound links this instance declares to peers.
    pub outbound_links: Vec<OutboundLink>,
    /// Hash/reference of the instance's capability policy (`policy.toml`),
    /// for local AccessController construction.
    pub capability_policy_ref: Option<String>,
    /// Cache/validation tag.
    pub etag: Option<String>,
}

/// `POST /federation/routed-write` request: Dominic asks an instance to apply
/// a write under its authoritative scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutedWrite {
    /// The target instance.
    pub target: InstanceId,
    /// The scope being written.
    pub scope: AuthoritativeScope,
    /// The write operation.
    pub operation: WriteOperation,
    /// The opaque write payload. Scaffold contract: a [`crate::Memory`]-shaped
    /// JSON object (the per-scope payload contract is a follow-on).
    pub payload: serde_json::Value,
}

/// `POST /federation/routed-write` receipt: Ijima's authoritative confirmation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutedWriteReceipt {
    /// Whether the write was accepted.
    pub accepted: bool,
    /// The instance that produced the receipt.
    pub instance: InstanceId,
    /// The scope written.
    pub scope: AuthoritativeScope,
    /// The resulting commit id, when accepted.
    pub commit: Option<String>,
    /// Non-fatal warnings (downgrades, scope narrowing, freshness, scaffold
    /// deferrals).
    pub warnings: Vec<String>,
}

/// `POST /federation/conflict-signal`: Ijima tells Dominic a conflict needs
/// adjudication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictSignal {
    /// The contested scope.
    pub scope: AuthoritativeScope,
    /// The instances in conflict.
    pub instances: Vec<InstanceId>,
    /// The conflict resolution Ijima applied (or requests).
    pub resolution: ConflictResolution,
    /// Human-readable detail.
    pub detail: Option<String>,
}

/// An instance's federation self-description — the source for
/// [`FederationState`]. Constructed at server startup (from config); the
/// single-instance default is `Unifying` / local scope / no links.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceFederationConfig {
    /// The instance's stable id.
    pub instance_id: InstanceId,
    /// The instance's federation role.
    pub role: InstanceRole,
    /// Scopes this instance is authoritative for.
    pub authoritative_scopes: Vec<AuthoritativeScope>,
    /// Outbound links this instance declares.
    pub outbound_links: Vec<OutboundLink>,
    /// Reference to the capability policy.
    pub capability_policy_ref: Option<String>,
}

impl InstanceFederationConfig {
    /// Render the config as a [`FederationState`] (the `GET /federation/state`
    /// payload). `etag` is left `None` (cache validation is a follow-on).
    #[must_use]
    pub fn to_state(&self) -> FederationState {
        FederationState {
            instance_id: self.instance_id.clone(),
            role: self.role,
            authoritative_scopes: self.authoritative_scopes.clone(),
            outbound_links: self.outbound_links.clone(),
            capability_policy_ref: self.capability_policy_ref.clone(),
            etag: None,
        }
    }
}

impl Default for InstanceFederationConfig {
    /// The single-instance 0.1.0 default: the local instance, `Unifying`,
    /// authoritative for the local namespace, no peer links.
    fn default() -> Self {
        Self {
            instance_id: InstanceId::local(),
            role: InstanceRole::Unifying,
            authoritative_scopes: vec![AuthoritativeScope::new("local", "*")],
            outbound_links: Vec::new(),
            capability_policy_ref: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wire-compat: the JSON shapes must match dominic-core's DTOs byte-for-byte
    /// (InstanceId as a plain string, AuthoritativeScope as {namespace,project},
    /// enums as PascalCase). This is the contract that lets Dominic's
    /// FederationClient deserialize Ijima's responses without a shared crate.
    #[test]
    fn federation_state_serializes_to_the_wire_contract() {
        let state = FederationState {
            instance_id: InstanceId::new("ijima-1"),
            role: InstanceRole::Unifying,
            authoritative_scopes: vec![AuthoritativeScope::new("shared", "Dominic")],
            outbound_links: vec![OutboundLink {
                target: InstanceId::new("ijima-2"),
                policy: LinkPolicy {
                    direction: LinkDirection::Replica,
                    conflict: ConflictResolution::SourceAuthority,
                    freshness: Freshness::Realtime,
                },
            }],
            capability_policy_ref: Some("sha256:abc".into()),
            etag: Some("w1".into()),
        };
        let json = serde_json::to_string(&state).expect("serialize");
        // InstanceId newtype → plain string (not {"0": ...})
        assert!(
            json.contains(r#""instance_id":"ijima-1""#),
            "InstanceId must serialize as a plain string: {json}"
        );
        // AuthoritativeScope → {namespace, project} struct
        assert!(
            json.contains(r#""authoritative_scopes":[{"namespace":"shared","project":"Dominic"}]"#),
            "AuthoritativeScope must be a struct: {json}"
        );
        // Enums → PascalCase
        assert!(json.contains(r#""role":"Unifying""#), "role: {json}");
        assert!(
            json.contains(r#""direction":"Replica""#),
            "direction: {json}"
        );
        assert!(
            json.contains(r#""conflict":"SourceAuthority""#),
            "conflict: {json}"
        );
        // round-trips
        let back: FederationState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(state, back);
    }

    #[test]
    fn config_default_is_local_unifying() {
        let cfg = InstanceFederationConfig::default();
        assert_eq!(cfg.instance_id, InstanceId::local());
        assert_eq!(cfg.role, InstanceRole::Unifying);
        assert!(cfg.outbound_links.is_empty());
        let state = cfg.to_state();
        assert_eq!(state.instance_id, InstanceId::local());
        assert_eq!(state.role, InstanceRole::Unifying);
        assert!(state.etag.is_none());
    }

    #[test]
    fn routed_write_and_receipt_round_trip() {
        let write = RoutedWrite {
            target: InstanceId::new("ijima-1"),
            scope: AuthoritativeScope::new("shared", "Dominic"),
            operation: WriteOperation::Create,
            payload: serde_json::json!({"content": "hello"}),
        };
        let json = serde_json::to_string(&write).expect("serialize");
        let back: RoutedWrite = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(write.target, back.target);
        assert_eq!(write.scope, back.scope);
        assert_eq!(write.operation, back.operation);

        let receipt = RoutedWriteReceipt {
            accepted: true,
            instance: InstanceId::new("ijima-1"),
            scope: AuthoritativeScope::new("shared", "Dominic"),
            commit: Some("mem_1".into()),
            warnings: vec![],
        };
        let rj = serde_json::to_string(&receipt).expect("serialize");
        let _: RoutedWriteReceipt = serde_json::from_str(&rj).expect("deserialize");
    }
}
