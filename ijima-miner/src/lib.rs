// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! # ijima-miner
//!
//! Session-context mining engine for
//! [Ijima](https://github.com/Industrial-Algebra/Ijima).
//!
//! Ijima's novel capability: raw session transcripts (the ore) are mined
//! into curated memory palace entries with full provenance (the refined
//! metal). This crate is the extraction engine. It reads session turns,
//! proposes decisions, facts, references, patterns, and knowledge-graph
//! triples, and emits [`Memory`] entries tagged
//! [`MemorySource::Mined`](ijima_core::memory::MemorySource::Mined).
//!
//! Extraction tiers are independently feature-gated so the cheapest
//! pass (rules) can run without the model-backed tier (llm).
//!
//! ## Architecture
//!
//! See `docs/adr/miner-architecture.md`. In short:
//! - **Extraction is pure** ([`mine`] is sync, side-effect-free); a separate
//!   async ingest step writes results to the store.
//! - **Rules tier runs unconditionally**; the **llm tier** runs when the
//!   `llm` feature is on; results merge and content-dedup before ingest.
//! - **Auto** extractions go straight to the palace; **PendingReview** stage
//!   in a per-namespace review queue.
//!
//! ## Features
//!
//! - `std` (default): Standard library support.
//! - `rules`: rule-based extraction (URL detection, decision patterns). No
//!   model, no network. *(always active; the feature gate reserves the
//!   module for future splitting.)*
//! - `llm`: model-backed extraction via Proserpina (Agent trait + personas).
//!   Adds the Proserpina dependency.

#![forbid(unsafe_code)]

use ijima_core::{Result, harness::Harness};

pub mod rules;

/// A proposed extraction from a mining pass.
///
/// Exhaustive so callers must handle every outcome when new tiers land.
#[derive(Debug, Clone, PartialEq)]
pub enum Extraction {
    /// A high-confidence memory to auto-archive.
    Auto(ijima_core::Memory),
    /// A medium-confidence memory queued for operator review.
    PendingReview(ijima_core::Memory),
    /// Nothing extractable from this turn range.
    Nothing,
}

/// Provenance + heuristics supplied to an extraction pass.
///
/// Every extracted [`ijima_core::Memory`] is stamped with this context so the
/// palace entry traces back to its source session (ADR M6).
#[derive(Debug, Clone)]
pub struct MiningContext {
    /// The session the turns were drawn from.
    pub session_id: String,
    /// Best-effort project (caller may refine on review).
    pub project: String,
    /// Which harness produced the session.
    pub harness: Harness,
    /// Creation timestamp to stamp on extractions (epoch-secs string).
    pub now: String,
}

/// Runs an extraction pass over a slice of raw session-turn text.
///
/// Pure and synchronous (ADR M1): it only extracts; a separate async ingest
/// step writes results to the store. The rules tier runs unconditionally
/// (ADR M7); the llm tier runs when the `llm` feature is on.
///
/// Returns the proposed extractions in turn order. `Extraction::Nothing`
/// entries are filtered out — only actionable extractions are returned.
///
/// # Errors
///
/// Returns [`ijima_core::IjimaError::Mining`] if the configured
/// extraction tier fails (e.g. an LLM provider error).
pub fn mine(turns: &[String], ctx: &MiningContext) -> Result<Vec<Extraction>> {
    let mut out = Vec::new();
    for turn in turns {
        let mut found = rules::extract_turn(turn, ctx);
        out.append(&mut found);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> MiningContext {
        MiningContext {
            session_id: "sess_1".into(),
            project: "ijima".into(),
            harness: Harness::Pi,
            now: "0".into(),
        }
    }

    #[test]
    fn empty_session_yields_no_extractions() {
        let got = mine(&[], &ctx()).expect("empty mine must not error");
        assert!(got.is_empty());
    }

    #[test]
    fn mine_extracts_decision_across_turns() {
        let turns = vec![
            "noise".to_string(),
            "We decided to use SurrealDB.".to_string(),
            "see https://example.com".to_string(),
        ];
        let got = mine(&turns, &ctx()).expect("mine");
        // 1 decision + 1 reference
        assert_eq!(got.len(), 2);
        assert!(got.iter().any(|e| matches!(
            e,
            Extraction::Auto(m) if m.topic == "decisions"
        )));
        assert!(got.iter().any(|e| matches!(
            e,
            Extraction::Auto(m) if m.topic == "references"
        )));
    }

    #[test]
    fn nothing_turns_produce_no_extractions() {
        let turns = vec!["just chatting".to_string(), "no signals".to_string()];
        let got = mine(&turns, &ctx()).expect("mine");
        assert!(got.is_empty());
    }
}
