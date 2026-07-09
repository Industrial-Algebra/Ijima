// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! The mining review queue (ADR M2, M3).
//!
//! PendingReview extractions stage here, per namespace, before promotion to
//! the memory palace. This keeps the palace (`store_memory`) clean of
//! unreviewed entries. An operator with `mining:review` lists pending
//! extractions, then accepts (→ promote to palace) or rejects (→ drop).

use crate::{Memory, MemoryId};

/// A staged extraction awaiting operator review.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct QueuedExtraction {
    /// Stable id within the queue (e.g. `q_<ulid>`).
    pub id: String,
    /// The proposed memory (MemorySource::Mined).
    pub memory: Memory,
    /// Extraction confidence (0.0–1.0); determined Auto/PendingReview routing
    /// and gives the reviewer context. Not stored on the promoted Memory.
    pub confidence: f32,
    /// The session the extraction was mined from (provenance for the reviewer).
    pub source_session_id: String,
    /// Epoch-secs string when the extraction was enqueued.
    pub queued_at: String,
}

/// Outcome of accepting a queued extraction: the promoted memory's id.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AcceptedExtraction {
    /// The memory id now living in the palace.
    pub memory_id: MemoryId,
}
