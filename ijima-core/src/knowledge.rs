// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! Knowledge graph domain types — entities (nodes) + temporal triples
//! (edges). The structured-facts layer: *"Who depends on Y?" "What did
//! we decide about X?"*
//!
//! Import-compatible with the pi-mempalace `entities` + `triples` schema
//! (see `docs/HANDOFF.md` §3). In the SurrealDB backend, entities are
//! record nodes and triples are graph edges (`RELATE ... ->triples->`).

use async_trait::async_trait;

use crate::{NamespaceId, Result};

/// A knowledge-graph triple prepared for bulk import — the wire shape
/// shared by `ijima import` (source dbs) and the HTTP client
/// (`import_kg`). Subject/object are entity **names** (Ijima's
/// id-is-name convention); `valid_to` is applied as a post-add
/// invalidation when present.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ImportTriple {
    /// Subject entity **name** (becomes the `EntityId`).
    pub subject: String,
    /// The relationship verb (e.g. `depends_on`).
    pub predicate: String,
    /// Object entity **name** (becomes the `EntityId`).
    pub object: String,
    /// When the fact became true (ISO date), if known.
    pub valid_from: Option<String>,
    /// When the fact stopped being true; applied as a post-add
    /// invalidation on import.
    pub valid_to: Option<String>,
    /// Source confidence `[0, 1]`.
    pub confidence: f32,
    /// The memory that evidenced the fact, if any.
    pub source_memory_id: Option<String>,
}

/// Aggregate counts reported by a knowledge-graph import run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct KgImportCounts {
    /// Triples read from the source.
    pub attempted: usize,
    /// Successfully added (including invalidations for historical
    /// ranges).
    pub added: usize,
    /// Not added — transport or store failure per-triple.
    pub skipped: usize,
}

/// Stable opaque identifier for an entity (node).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct EntityId(pub String);

impl EntityId {
    /// Construct an entity id.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The stable wire string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An entity — a node in the knowledge graph.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Entity {
    /// Stable opaque identifier (e.g. `Quantizon`, `candle`).
    pub id: EntityId,
    /// Human-readable display name.
    pub name: String,
    /// Semantic type (`"project"`, `"person"`, `"tool"`, `"unknown"`...).
    pub entity_type: String,
    /// Owning namespace (isolation).
    pub namespace: String,
}

/// A temporal triple: subject -predicate-> object with validity. The
/// `valid_to` field expresses invalidation (`None` = still current).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Triple {
    /// The edge record id (backend-assigned).
    pub id: String,
    /// The source entity.
    pub subject: EntityId,
    /// The relationship (`"depends_on"`, `"uses"`, `"decided"`...).
    pub predicate: String,
    /// The target entity.
    pub object: EntityId,
    /// When the fact became true (epoch-secs string).
    pub valid_from: Option<String>,
    /// When it stopped being true (`None` = still current).
    pub valid_to: Option<String>,
    /// Confidence score (0.0–1.0).
    pub confidence: f32,
    /// Owning namespace.
    pub namespace: String,
    /// The memory this fact was extracted from, when known.
    pub source_memory_id: Option<String>,
}

/// An entity plus its connected triples (query result).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EntityRecord {
    /// The entity node, if it exists.
    pub entity: Option<Entity>,
    /// Triples where this entity is the subject (outgoing).
    pub outgoing: Vec<Triple>,
    /// Triples where this entity is the object (incoming).
    pub incoming: Vec<Triple>,
}

/// Counts for the knowledge-graph stats endpoint.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct KgStats {
    /// Number of entity nodes.
    pub entities: usize,
    /// Number of triple edges.
    pub triples: usize,
}

/// The knowledge-graph contract every backend implements alongside
/// [`crate::Store`]. All methods are scoped to a [`NamespaceId`].
#[async_trait]
pub trait KnowledgeGraph: Send + Sync {
    /// Adds (or refreshes) the subject + object entities and creates a
    /// triple edge between them. Returns the created [`Triple`].
    #[allow(clippy::too_many_arguments)]
    async fn add_triple(
        &self,
        ns: &NamespaceId,
        subject: EntityId,
        predicate: &str,
        object: EntityId,
        valid_from: Option<&str>,
        confidence: f32,
        source_memory_id: Option<&str>,
    ) -> Result<Triple>;

    /// Returns an entity and all its connected triples (outgoing +
    /// incoming) within `ns`.
    async fn query_entity(&self, ns: &NamespaceId, entity: &EntityId) -> Result<EntityRecord>;

    /// Marks a triple as no longer current by setting `valid_to`.
    /// Idempotent.
    async fn invalidate_triple(&self, ns: &NamespaceId, triple_id: &str) -> Result<()>;

    /// Finds triples matching any combination of subject / predicate /
    /// object (`None` = wildcard).
    async fn find_triples(
        &self,
        ns: &NamespaceId,
        subject: Option<&EntityId>,
        predicate: Option<&str>,
        object: Option<&EntityId>,
    ) -> Result<Vec<Triple>>;

    /// Returns triples in chronological order (by `valid_from`), most
    /// recent first.
    async fn kg_timeline(&self, ns: &NamespaceId, limit: usize) -> Result<Vec<Triple>>;

    /// Entity + triple counts for `ns`.
    async fn knowledge_stats(&self, ns: &NamespaceId) -> Result<KgStats>;

    /// Global entity + triple counts across all namespaces.
    async fn kg_global_stats(&self) -> Result<KgStats>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_id_round_trips() {
        let id = EntityId::new("Quantizon");
        assert_eq!(id.as_str(), "Quantizon");
        assert_eq!(id, EntityId("Quantizon".into()));
    }

    #[test]
    fn kg_stats_defaults_to_zero() {
        let s = KgStats::default();
        assert_eq!(s.entities, 0);
        assert_eq!(s.triples, 0);
    }
}
