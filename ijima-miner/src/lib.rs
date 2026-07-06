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
//! ## Features
//!
//! - `std` (default): Standard library support.
//! - `rules`: cheap rule-based extraction (URL detection, decision
//!   pattern matching). No model, no network.
//! - `llm`: model-backed extraction via an OpenAI-compatible HTTP
//!   provider (DeepSeek by default, configurable). Adds an async HTTP
//!   client.
//!
//! ## Status
//!
//! Scaffolded — the rule extractor lands first (TDD), then the LLM tier.

#![forbid(unsafe_code)]

use ijima_core::Result;

/// A proposed extraction from a mining pass.
///
/// Exhaustive so callers must handle every outcome when new tiers land.
#[derive(Debug, Clone, PartialEq)]
pub enum Extraction {
    /// A high-confidence memory to auto-archive.
    Auto(ijima_core::Memory),
    /// A medium-confidence memory queued for operator review (via Tsume's
    /// dashboard).
    PendingReview(ijima_core::Memory),
    /// Nothing extractable from this turn range.
    Nothing,
}

/// Runs an extraction pass over a slice of raw session-turn text.
///
/// This is the unified entry point; the active tier(s) are selected by
/// feature flags. Returns the proposed extractions in turn order.
///
/// # Errors
///
/// Returns [`ijima_core::IjimaError::Mining`] if the configured
/// extraction tier fails (e.g. an LLM provider error).
pub fn mine(_turns: &[String]) -> Result<Vec<Extraction>> {
    // TDD: the first failing test feeds a "we decided X" line and
    // expects a Mined extraction. Implementation follows.
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_session_yields_no_extractions() {
        let got = mine(&[]).expect("empty mine must not error");
        assert!(got.is_empty());
    }
}
