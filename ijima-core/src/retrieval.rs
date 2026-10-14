// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! The retrieval ledger (v0.4.2): an append-only record of what the
//! read paths actually serve. Every wake-up, search, and browse
//! response appends one [`RetrievalEvent`] per served memory, so
//! admission, starvation, and per-memory frequency are answerable
//! after the fact. Events carry ids and metadata only — never memory
//! content and never query text.

use serde::{Deserialize, Serialize};

/// Which read path served the memory, and (for wake-up) from which
/// stratum. Serialized PascalCase like the crate's other enums.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RetrievalKind {
    /// Wake-up recency stratum (admitted first; the starvation fix).
    WakeupRecent,
    /// Wake-up lexicographic fill (importance-order after the recency
    /// stratum).
    WakeupLexicographic,
    /// The universal doctrine wall in wake-up.
    WakeupDoctrine,
    /// Semantic search (`POST /memories/search`).
    Search,
    /// Browse/list (`GET /memories`).
    Browse,
}

/// One served memory. `rank` is the 0-based position in the response
/// list it was served in; `similarity` is set for `Search` events only
/// (cosine similarity, 0.0–1.0) and absent otherwise.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievalEvent {
    /// Groups all events from one request.
    pub retrieval_id: String,
    /// The read path (and stratum, for wake-up).
    pub kind: RetrievalKind,
    /// The wall the memory was served from. For `scope=visible`
    /// search this is the contributing wall each hit merged from.
    pub namespace: String,
    /// The served memory's id (join key back to the palace).
    pub memory_id: String,
    /// The authenticated principal that triggered the retrieval.
    pub principal: String,
    /// 0-based position in the served list.
    pub rank: u32,
    /// Cosine similarity; present on `Search` events only. Skipped
    /// when `None` (a serialized null here would poison reads — the
    /// v0.4.1 lesson).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub similarity: Option<f32>,
    /// Unix seconds.
    pub ts_unix: u64,
}

/// Aggregated ledger counts (U2): what the read paths served, by kind.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalStats {
    /// Total events counted.
    pub total: u64,
    /// Events by kind. Keys are the PascalCase kind names.
    pub by_kind: std::collections::BTreeMap<String, u64>,
    /// Distinct memory ids served at least once.
    pub distinct_memories: u64,
    /// Distinct retrieval ids (i.e. requests served).
    pub distinct_retrievals: u64,
    /// Oldest event ts (Unix seconds); 0 when empty.
    pub oldest_ts_unix: u64,
    /// Newest event ts (Unix seconds); 0 when empty.
    pub newest_ts_unix: u64,
}

// The ledger's JSON shape is exercised through `serde_json`, which
// ijima-core only links under `federation` (the crate's established
// gating for JSON-dependent tests — see `memory.rs`).

// serde_json is exposed by ijima-core only through the `federation`
// feature (the crate's existing convention — see memory.rs), so the
// round-trip tests ride that gate.
#[cfg(all(test, feature = "federation"))]
mod tests {
    use super::*;

    #[test]
    fn event_round_trips_pascal_case_kind_and_skipped_similarity() {
        let e = RetrievalEvent {
            retrieval_id: "r1".into(),
            kind: RetrievalKind::WakeupRecent,
            namespace: "ns_x".into(),
            memory_id: "m1".into(),
            principal: "elliott".into(),
            rank: 0,
            similarity: None,
            ts_unix: 100,
        };
        let j = serde_json::to_string(&e).expect("serialize");
        assert!(!j.contains("similarity"), "None similarity is skipped: {j}");
        assert!(j.contains("\"WakeupRecent\""), "PascalCase kind: {j}");
        let back: RetrievalEvent = serde_json::from_str(&j).expect("deserialize");
        assert_eq!(back, e);
    }

    #[test]
    fn event_round_trips_with_similarity() {
        let e = RetrievalEvent {
            retrieval_id: "r2".into(),
            kind: RetrievalKind::Search,
            namespace: "ns_x".into(),
            memory_id: "m2".into(),
            principal: "elliott".into(),
            rank: 3,
            similarity: Some(0.75),
            ts_unix: 100,
        };
        let j = serde_json::to_string(&e).expect("serialize");
        assert!(j.contains("\"similarity\":0.75"), "{j}");
        let back: RetrievalEvent = serde_json::from_str(&j).expect("deserialize");
        assert_eq!(back, e);
    }
}
