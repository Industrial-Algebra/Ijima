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
use crate::provenance::{AuthorityScope, InstanceId};

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
    /// Provenance: the instance that authored this entry (ADR
    /// provenance-tier). Defaults to the local instance for 0.1.0;
    /// federation (Phase 5) stamps the origin instance.
    #[cfg_attr(feature = "serde", serde(default))]
    pub origin: InstanceId,
    /// Provenance: the authority scope (source-of-truth) for this entry's
    /// domain (ADR provenance-tier). Defaults to local; drives Phase 5
    /// conflict resolution.
    #[cfg_attr(feature = "serde", serde(default))]
    pub authority: AuthorityScope,
    /// Importance score (0.0–1.0). Wake-up ranks lexicographically:
    /// `importance DESC, created_at DESC` — recency breaks ties within
    /// equal importance only; it never closes an importance gap. (The
    /// former "importance × recency" claim here was drifted prose — see
    /// RABBIT_HOLE_2026-09-22_Ijima; ranking semantics are a 0.4 design
    /// question.) Defaults to 0.5, matching pi-mempalace.
    #[cfg_attr(feature = "serde", serde(default = "default_importance"))]
    pub importance: f32,
    /// Evidence grade (direction D). Defaults to `Interpreted` — legacy
    /// rows and ungraded saves are interpretations, never observed fact.
    #[cfg_attr(feature = "serde", serde(default))]
    pub evidence: EvidenceGrade,
    /// Citations grounding an `Observed` grade. Validated non-empty at
    /// save time (see [`Memory::validate_evidence`]); meaningless (and
    /// typically empty) for `Interpreted`.
    #[cfg_attr(feature = "serde", serde(default))]
    pub citations: Vec<Citation>,
    /// Correction link (v0.4.0): the id of the memory this one supersedes
    /// ("this replaces that"). Set once at save time; the server writes
    /// the inverse link onto the target atomically with the insert.
    /// Chains forward only (A ← B ← C); re-superseding a superseded
    /// memory is rejected — supersede the successor instead.
    #[cfg_attr(feature = "serde", serde(default))]
    pub supersedes: Option<String>,
    /// Inverse correction link, written by the server: the id of the
    /// memory that superseded this one. Exclusion queries filter on this.
    #[cfg_attr(feature = "serde", serde(default))]
    pub superseded_by: Option<String>,
    /// When the supersede link landed (unix seconds), for audit.
    #[cfg_attr(feature = "serde", serde(default))]
    pub superseded_at_unix: Option<i64>,
    /// Creation timestamp. v0: Unix epoch seconds as a string (monotonic
    /// for DESC ordering). Future: ISO-8601 when a time crate lands.
    #[cfg_attr(feature = "serde", serde(default))]
    pub created_at: String,
}

/// Evidence grade (direction D, v0.4.0): is this content a claim the
/// authoring process directly observed, or an interpretation it formed?
/// Composes with the provenance tiers — the grade crosses tier lines
/// (an explicit save can still be an interpretation), so it is its own
/// axis. Defaults to `Interpreted`: ungraded and legacy rows are the
/// weaker claim, never masquerading as observed fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum EvidenceGrade {
    /// Directly observed behavior/event: the authoring process witnessed
    /// the thing itself (a session transcript event, a command output, a
    /// git artifact). Requires at least one [`Citation`].
    Observed,
    /// An interpretation, inference, claim, or summary formed about
    /// artifacts or events. No citation requirement — attribution is the
    /// grade itself.
    #[default]
    Interpreted,
}

/// A typed pointer to the artifact that grounds an [`EvidenceGrade::Observed`]
/// memory. "No citation, no candidate" — observed claims cite or they do
/// not ship.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Citation {
    /// What kind of artifact the locator points at.
    pub kind: CitationKind,
    /// Opaque locator: commit sha, report path, session id, file path, URL.
    pub locator: String,
}

/// The artifact kinds Ijima knows how to cite (v0.4.0 set).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CitationKind {
    /// A git commit sha.
    Commit,
    /// A path (or id) into the report corpus.
    Report,
    /// An Ijima session id.
    Session,
    /// A file path.
    File,
    /// A URL.
    Url,
}

impl Memory {
    /// Direction D invariant: an `Observed` memory must carry at least one
    /// citation. Returns a human-readable error string (mapped to 400 at
    /// the API layer) when violated.
    pub fn validate_evidence(&self) -> Result<(), String> {
        if self.evidence == EvidenceGrade::Observed && self.citations.is_empty() {
            Err(format!(
                "memory {} claims EvidenceGrade::Observed but cites nothing — \
                 observed claims require >= 1 citation",
                self.id.0
            ))
        } else {
            Ok(())
        }
    }
}

#[cfg(feature = "serde")]
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

impl MemorySource {
    /// The Schubert trust grade (codimension) of this tier — the
    /// quantitative trust axis. **Higher = more trusted.** Phase 5 egress
    /// filters turn this into intersection arithmetic ("does this
    /// content's grade fit the link's trust budget?"); Phase 4 keys
    /// doctrine-health on it.
    ///
    /// Grades fit comfortably inside Gr(4,8)'s 4×4 Schubert box:
    ///
    /// | Tier | `trust_grade` |
    /// |---|---|
    /// | [`AutoCapture`](MemorySource::AutoCapture) | 1 (ambient, unverified) |
    /// | [`Mined`](MemorySource::Mined) | 2 (model-extracted, review-eligible) |
    /// | [`Explicit`](MemorySource::Explicit) | 3 (a human deliberately saved it) |
    /// | [`Doctrine`](MemorySource::Doctrine) | 4 (Git-versioned, PR-reviewed, curated) |
    pub fn trust_grade(self) -> u64 {
        match self {
            MemorySource::AutoCapture => 1,
            MemorySource::Mined => 2,
            MemorySource::Explicit => 3,
            MemorySource::Doctrine => 4,
        }
    }
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
            origin: InstanceId::local(),
            authority: AuthorityScope::local(),
            importance: 0.8,
            evidence: EvidenceGrade::Observed,
            citations: vec![Citation {
                kind: CitationKind::Commit,
                locator: "abc123".into(),
            }],
            supersedes: None,
            superseded_by: None,
            superseded_at_unix: None,
            created_at: "123".into(),
        };
        assert_eq!(m.id.0, "mem_01");
        assert_eq!(m.source, MemorySource::Mined);
        assert_eq!(m.harness, Harness::Pi);
        assert_eq!(m.importance, 0.8);
        assert_eq!(m.created_at, "123");
        assert_eq!(m.session_id.as_deref(), Some("sess_7"));
        assert_eq!(m.evidence, EvidenceGrade::Observed);
        assert_eq!(m.citations.len(), 1);
    }

    #[cfg(feature = "federation")]
    #[test]
    fn legacy_json_deserializes_as_interpreted() {
        let m: Memory = serde_json::from_str(
            r#"{"id":"mem_x","content":"c","project":"p","topic":"t","source":"Mined","harness":"Pi","importance":0.5,"created_at":"1"}"#,
        )
        .expect("legacy JSON must deserialize");
        assert_eq!(m.evidence, EvidenceGrade::Interpreted);
        assert!(m.citations.is_empty());
    }

    #[cfg(feature = "federation")]
    #[test]
    fn supersede_fields_default_none() {
        // Unit-1 JSON (evidence grade + citations present) predates the
        // supersede links: all three must deserialize as `None`.
        let m: Memory = serde_json::from_str(
            r#"{"id":"mem_legacy","content":"c","project":"p","topic":"t","source":"Mined","harness":"Pi","importance":0.5,"evidence":"Observed","citations":[{"kind":"Commit","locator":"abc123"}],"created_at":"1"}"#,
        )
        .expect("Unit-1 JSON must deserialize");
        assert!(m.supersedes.is_none());
        assert!(m.superseded_by.is_none());
        assert!(m.superseded_at_unix.is_none());
    }

    #[test]
    fn validate_evidence_rejects_observed_without_citations() {
        let m = Memory {
            id: MemoryId("mem_uncited".into()),
            content: "c".into(),
            project: "p".into(),
            topic: "t".into(),
            source: MemorySource::Explicit,
            harness: Harness::Pi,
            session_id: None,
            origin: InstanceId::local(),
            authority: AuthorityScope::local(),
            importance: 0.5,
            evidence: EvidenceGrade::Observed,
            citations: Vec::new(),
            supersedes: None,
            superseded_by: None,
            superseded_at_unix: None,
            created_at: "1".into(),
        };
        let err = m.validate_evidence().expect_err("must reject");
        assert!(err.contains("mem_uncited"), "error names the memory: {err}");
    }

    #[test]
    fn validate_evidence_accepts_observed_with_citation() {
        let m = Memory {
            id: MemoryId("mem_cited".into()),
            content: "c".into(),
            project: "p".into(),
            topic: "t".into(),
            source: MemorySource::Explicit,
            harness: Harness::Pi,
            session_id: None,
            origin: InstanceId::local(),
            authority: AuthorityScope::local(),
            importance: 0.5,
            evidence: EvidenceGrade::Observed,
            citations: vec![Citation {
                kind: CitationKind::Commit,
                locator: "abc123".into(),
            }],
            supersedes: None,
            superseded_by: None,
            superseded_at_unix: None,
            created_at: "1".into(),
        };
        assert!(m.validate_evidence().is_ok());
    }

    #[test]
    fn validate_evidence_accepts_interpreted_without_citations() {
        let m = Memory {
            id: MemoryId("mem_interp".into()),
            content: "c".into(),
            project: "p".into(),
            topic: "t".into(),
            source: MemorySource::Mined,
            harness: Harness::Pi,
            session_id: None,
            origin: InstanceId::local(),
            authority: AuthorityScope::local(),
            importance: 0.5,
            evidence: EvidenceGrade::Interpreted,
            citations: Vec::new(),
            supersedes: None,
            superseded_by: None,
            superseded_at_unix: None,
            created_at: "1".into(),
        };
        assert!(m.validate_evidence().is_ok());
    }

    #[cfg(feature = "federation")]
    #[test]
    fn evidence_grade_serializes_pascalcase() {
        let json = serde_json::to_string(&EvidenceGrade::Observed).expect("serialize");
        assert_eq!(json, "\"Observed\"");
    }

    #[test]
    fn doctrine_is_distinct_from_explicit() {
        // Doctrine is the curated, Git-versioned tier — it must not be
        // confused with an operator's explicit save.
        assert!(matches!(MemorySource::Doctrine, MemorySource::Doctrine));
        assert_ne!(MemorySource::Doctrine, MemorySource::Explicit);
        assert_ne!(MemorySource::Doctrine, MemorySource::Mined);
    }

    #[test]
    fn trust_grade_monotone_in_trust() {
        // Higher tier ⇒ higher grade (more trusted). Drives Phase 5 egress
        // intersection arithmetic and Phase 4 doctrine-health.
        assert_eq!(MemorySource::AutoCapture.trust_grade(), 1);
        assert_eq!(MemorySource::Mined.trust_grade(), 2);
        assert_eq!(MemorySource::Explicit.trust_grade(), 3);
        assert_eq!(MemorySource::Doctrine.trust_grade(), 4);
        assert!(
            MemorySource::AutoCapture.trust_grade() < MemorySource::Mined.trust_grade()
                && MemorySource::Mined.trust_grade() < MemorySource::Explicit.trust_grade()
                && MemorySource::Explicit.trust_grade() < MemorySource::Doctrine.trust_grade()
        );
    }
}
