// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! Memory palace domain types — the curated long-term memory surface.
//!
//! These types are import-compatible with the pi-mempalace schema (see
//! `docs/HANDOFF.md` §3 for the live SQLite schema). The store
//! implementation lives in `ijima-server`; this module defines only the
//! pure domain model so it can be shared by the server, miner, and
//! client crates.

use crate::harness::Harness;

/// A newtype for the stable, opaque identifier of a stored memory.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct MemoryId(pub String);

/// A curated memory palace entry: the refined-metal output of either an
/// explicit save or a mining pass.
///
/// Provenance (`harness`, `session_id`, `source`) is mandatory so that
/// any entry can be traced back to the conversation that produced it.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Memory {
    /// Stable, opaque identifier (e.g. `mem_<ulid>`).
    pub id: MemoryId,
    /// The curated content text.
    pub content: String,
    /// Project namespace (defaults to `"general"`).
    pub project: String,
    /// Topic within the project.
    pub topic: String,
    /// Provenance: how this entry was created — explicit save, auto-capture,
    /// or a mined extraction.
    pub source: MemorySource,
    /// Provenance: which harness wrote this entry.
    pub harness: Harness,
    /// Provenance: the originating session, when known.
    pub session_id: Option<String>,
    /// Importance score (0.0–1.0). Used for wake-up ranking
    /// (top-N by importance × recency). Defaults to 0.5, matching
    /// pi-mempalace.
    #[cfg_attr(feature = "serde", serde(default = "default_importance"))]
    pub importance: f32,
    /// Creation timestamp. v0: Unix epoch seconds as a string (monotonic
    /// for DESC ordering). Future: ISO-8601 when a time crate lands.
    #[cfg_attr(feature = "serde", serde(default))]
    pub created_at: String,
}

fn default_importance() -> f32 {
    0.5
}

/// How a [`Memory`] entered the palace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MemorySource {
    /// An operator or harness explicitly saved it.
    Explicit,
    /// An auto-capture hook wrote it.
    AutoCapture,
    /// The miner extracted it from a session transcript.
    Mined,
    /// Doctrine: curated, Git-versioned, PR-reviewed memory mirrored from
    /// the repository seed pack. Never written directly by agents — the
    /// highest-trust, lowest-write-rate tier.
    Doctrine,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_round_trips_its_fields() {
        let m = Memory {
            id: MemoryId("mem_01".into()),
            content: "Decided to use DeepSeek for extraction.".into(),
            project: "ijima".into(),
            topic: "mining".into(),
            source: MemorySource::Mined,
            harness: Harness::Pi,
            session_id: Some("sess_7".into()),
            importance: 0.8,
            created_at: "123".into(),
        };
        assert_eq!(m.id.0, "mem_01");
        assert_eq!(m.source, MemorySource::Mined);
        assert_eq!(m.harness, Harness::Pi);
        assert_eq!(m.importance, 0.8);
        assert_eq!(m.created_at, "123");
        assert_eq!(m.session_id.as_deref(), Some("sess_7"));
    }

    #[test]
    fn doctrine_is_distinct_from_explicit() {
        // Doctrine is the curated, Git-versioned tier — it must not be
        // confused with an operator's explicit save.
        assert!(matches!(MemorySource::Doctrine, MemorySource::Doctrine));
        assert_ne!(MemorySource::Doctrine, MemorySource::Explicit);
        assert_ne!(MemorySource::Doctrine, MemorySource::Mined);
    }
}
