// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! Ijima's capability vocabulary.
//!
//! These stable wire identifiers map onto Schubert's capability model.
//! The geometric policy — Grassmannian, partitions, capability kinds, and
//! principal grants — lives in
//! [`policy/policy.toml`](../../policy/policy.toml) at the repository root
//! and is loaded by [`ijima_server::auth`] via Schubert's `policy`
//! feature.
//!
//! ## Policy selection (via Schubert's recommender)
//!
//! Ijima's access-control constraints were fed to Schubert's
//! `recommend` CLI (5 roles, 3 namespaces, audit + crypto + policy
//! required, discrete trust, ~50 principals). It selected:
//!
//! - **Grassmannian Gr(4,8)**, policy dimension `k(n-k) = 16`
//!   (Schubert's enterprise / multi-tenant bucket).
//! - **Features**: `std`, `crypto`, `policy`.
//! - **Computation path**: LR.
//!
//! The `policy` feature means the vocabulary is declarative TOML, not
//! hardcoded Rust — see `policy/policy.toml`.
//!
//! ## Vocabulary (on Gr(4,8), partitions fit a 4×4 box)
//!
//! | Capability ID | Kind | Partition | Codim | Grants |
//! |---|---|---|---|---|
//! | [`MEMORY_READ`] | ReadLike | σ₁ | 1 | read memory palace entries |
//! | [`KNOWLEDGE_READ`] | ReadLike | σ₁ | 1 | query entities/triples/timeline |
//! | [`MINING_REVIEW`] | ReadLike | σ₂ | 2 | read + accept/reject the review queue |
//! | [`MEMORY_WRITE`] | WriteLike | σ₂ | 2 | store palace entries (dedup-aware) |
//! | [`KNOWLEDGE_WRITE`] | WriteLike | σ₂ | 2 | add/invalidate triples |
//! | [`SESSION_INGEST`] | WriteLike | σ₃ | 3 | append session-context turns |
//! | [`MINING_TRIGGER`] | WriteLike | σ₃₁ | 4 | trigger an extraction pass |
//! | [`ADMIN`] | AdminLike | σ₄₄₄₄ (point) | 16 | full control |

/// The Grassmannian Ijima's policy lives on: **Gr(4,8)**, dimension 16.
/// Selected by Schubert's recommender for Ijima's multi-tenant
/// (3-namespace, 5-role) constraint set.
pub const POLICY_GRASSMANNIAN: (usize, usize) = (4, 8);

/// Read memory palace entries.
pub const MEMORY_READ: &str = "memory:read";
/// Query entities, triples, and the knowledge-graph timeline.
pub const KNOWLEDGE_READ: &str = "knowledge:read";
/// Read and accept/reject the mining review queue.
pub const MINING_REVIEW: &str = "mining:review";
/// Store palace entries (dedup-aware).
pub const MEMORY_WRITE: &str = "memory:write";
/// Add or invalidate knowledge-graph triples.
pub const KNOWLEDGE_WRITE: &str = "knowledge:write";
/// Append raw session-context turns to the repository.
pub const SESSION_INGEST: &str = "session:ingest";
/// Trigger a mining/extraction pass over session context.
pub const MINING_TRIGGER: &str = "mining:trigger";
/// Full administrative control (the point class σ₄₄₄₄; implies all others).
pub const ADMIN: &str = "admin";

/// Every capability wire ID, in increasing-codimension order. Used to
/// validate identifiers at the API boundary; the geometric definitions
/// live in `policy/policy.toml`.
pub const ALL_CAPABILITIES: &[&str] = &[
    MEMORY_READ,
    KNOWLEDGE_READ,
    MINING_REVIEW,
    MEMORY_WRITE,
    KNOWLEDGE_WRITE,
    SESSION_INGEST,
    MINING_TRIGGER,
    ADMIN,
];

/// The Schubert intersection number (codimension) of a capability's
/// partition — the geometric weight used for rate-limiting capacity.
///
/// A capability's codimension is the sum of its partition parts (the
/// degree of its Schubert cycle). Per Schubert's rate-limiter, this
/// becomes the per-principal token-bucket capacity multiplier: a
/// principal holding `memory:write` (codim 2) gets 2× the throughput of
/// one holding `memory:read` (codim 1); `admin` (the point class, codim
/// 16) gets 16×. *The geometry of access maps to the geometry of
/// throughput.*
///
/// **Coupling:** these codimensions mirror `policy/policy.toml`. If the
/// policy's partitions change, update this mapping. Unknown capability
/// ids default to codim 1 (the lowest, σ₁) so a future capability is
/// rate-limited conservatively until promoted here.
pub fn intersection_number(capability: &str) -> u64 {
    match capability {
        MEMORY_READ | KNOWLEDGE_READ => 1,
        MINING_REVIEW | MEMORY_WRITE | KNOWLEDGE_WRITE => 2,
        SESSION_INGEST => 3,
        MINING_TRIGGER => 4,
        ADMIN => 16,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn all_capability_ids_are_unique() {
        let set: HashSet<&&str> = ALL_CAPABILITIES.iter().collect();
        assert_eq!(set.len(), ALL_CAPABILITIES.len(), "duplicate capability id");
    }

    #[test]
    fn policy_grassmannian_is_valid_schubert_space() {
        let (k, n) = POLICY_GRASSMANNIAN;
        // Schubert requires 0 < k < n.
        assert!(k > 0 && k < n);
        // Must match the recommender-selected enterprise/multi-tenant preset.
        assert_eq!(k * (n - k), 16);
    }

    #[test]
    fn admin_is_in_vocabulary() {
        assert!(ALL_CAPABILITIES.contains(&ADMIN));
    }

    #[test]
    fn intersection_numbers_match_policy_codimensions() {
        // Codimension = sum of partition parts (mirrors policy/policy.toml).
        assert_eq!(intersection_number(MEMORY_READ), 1);
        assert_eq!(intersection_number(KNOWLEDGE_READ), 1);
        assert_eq!(intersection_number(MINING_REVIEW), 2);
        assert_eq!(intersection_number(MEMORY_WRITE), 2);
        assert_eq!(intersection_number(KNOWLEDGE_WRITE), 2);
        assert_eq!(intersection_number(SESSION_INGEST), 3);
        assert_eq!(intersection_number(MINING_TRIGGER), 4);
        assert_eq!(intersection_number(ADMIN), 16);
        // Unknown capabilities default to the lowest codim (conservative).
        assert_eq!(intersection_number("future:cap"), 1);
    }
}
