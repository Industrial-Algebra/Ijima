// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! Rule-based extraction tier (ADR M7, M8).
//!
//! Cheap, deterministic, no model. Ships two roles:
//! - **Decision**: matches "we decided X", "let's go with Y", "agreed on Z"
//!   patterns and extracts the clause.
//! - **Reference**: detects URLs / links.
//!
//! All rule extractions route to [`crate::Extraction::Auto`] (high confidence —
//! the signal is unambiguous). The LLM tier adds Fact + Pattern roles.

use ijima_core::{AuthorityScope, InstanceId, Memory, MemoryId, MemorySource};

use crate::{Extraction, MiningContext};

/// Extracts decisions and references from a single turn.
///
/// `ctx` supplies the provenance (session id, harness) stamped onto every
/// extracted memory. Returns one [`Extraction`] per signal found, in
/// document order.
pub fn extract_turn(text: &str, ctx: &MiningContext) -> Vec<Extraction> {
    let mut out = Vec::new();
    out.extend(extract_decisions(text, ctx));
    out.extend(extract_references(text, ctx));
    out
}

/// Decision patterns. Each entry is (trigger phrase, trailing clause regex).
///
/// The trigger is matched case-insensitively at a word boundary; the clause
/// is the remainder of the sentence (up to `.`, `!`, `?`, or end of line).
/// Ordered most-specific first so "let's go with" beats a generic "go".
const DECISION_PHRASES: &[&str] = &[
    "we decided",
    "we've decided",
    "decided to",
    "let's go with",
    "lets go with",
    "let's use",
    "lets use",
    "we'll use",
    "agreed to",
    "agreed on",
    "settled on",
    "going with",
    "opted for",
    "chose to",
];

/// Extracts decision clauses. One extraction per non-overlapping trigger
/// hit (first-earliest wins, so "we decided" absorbs a later "decided to"
/// on the same sentence — ADR M7 dedup).
fn extract_decisions(text: &str, ctx: &MiningContext) -> Vec<Extraction> {
    let lower = text.to_ascii_lowercase();
    // Collect (trigger_start, clause_start, clause) for every word-boundary hit.
    let mut hits: Vec<(usize, usize, String)> = Vec::new();
    for phrase in DECISION_PHRASES {
        let mut search_from = 0;
        while let Some(rel) = lower[search_from..].find(phrase) {
            let abs = search_from + rel;
            if abs > 0 {
                let prev = lower.as_bytes()[abs - 1];
                if prev.is_ascii_alphanumeric() || prev == b'_' {
                    search_from = abs + phrase.len();
                    continue;
                }
            }
            let clause_start = abs + phrase.len();
            let clause = take_clause(&text[clause_start.min(text.len())..]);
            if !clause.is_empty() {
                hits.push((abs, clause_start, clause));
            }
            search_from = clause_start;
        }
    }
    // Earliest trigger wins; drop any hit whose clause starts inside an
    // already-emitted clause's span (overlapping phrases on one sentence).
    hits.sort_by_key(|(start, _, _)| *start);
    let mut out = Vec::new();
    let mut covered_until = 0usize; // end of the last emitted clause's source span
    for (_, clause_start, clause) in hits {
        if clause_start < covered_until {
            continue; // overlaps an already-emitted clause
        }
        covered_until = clause_start + clause.len();
        out.push(Extraction::Auto(decision_memory(clause.as_str(), ctx)));
    }
    out
}

/// Takes a clause up to the first sentence terminator, trimmed.
fn take_clause(rest: &str) -> String {
    let end = rest.find(['.', '!', '?', '\n']).unwrap_or(rest.len());
    let clause = rest[..end].trim().trim_end_matches(',').trim();
    clause.to_string()
}

/// Builds a Memory for a decision extraction.
fn decision_memory(clause: &str, ctx: &MiningContext) -> Memory {
    Memory {
        id: MemoryId(format!("mined_decision_{}", short_hash(clause))),
        content: format!("Decision: {clause}"),
        project: ctx.project.clone(),
        topic: "decisions".to_string(),
        source: MemorySource::Mined,
        harness: ctx.harness,
        session_id: Some(ctx.session_id.clone()),
        origin: InstanceId::local(),
        authority: AuthorityScope::local(),
        importance: 0.7,
        created_at: ctx.now.clone(),
    }
}

/// Reference patterns. A URL is `http(s)://...` or a bare `scheme:` link, up
/// to the first whitespace. We also catch `www.` hosts.
fn extract_references(text: &str, ctx: &MiningContext) -> Vec<Extraction> {
    let mut out = Vec::new();
    for token in text.split_whitespace() {
        let url = token.trim_end_matches([',', ')', ']', '.', ';']);
        if is_url(url) {
            out.push(Extraction::Auto(reference_memory(url, ctx)));
        }
    }
    out
}

/// Returns true if `s` looks like a URL/link worth recording.
fn is_url(s: &str) -> bool {
    s.starts_with("http://")
        || s.starts_with("https://")
        || s.starts_with("www.")
        || (s.contains("://") && s.len() > 8)
}

fn reference_memory(url: &str, ctx: &MiningContext) -> Memory {
    Memory {
        id: MemoryId(format!("mined_ref_{}", short_hash(url))),
        project: ctx.project.clone(),
        topic: "references".to_string(),
        content: format!("Reference: {url}"),
        source: MemorySource::Mined,
        harness: ctx.harness,
        session_id: Some(ctx.session_id.clone()),
        origin: InstanceId::local(),
        authority: AuthorityScope::local(),
        importance: 0.5,
        created_at: ctx.now.clone(),
    }
}

/// A short, stable, non-cryptographic hash for memory ids (keeps ids short).
fn short_hash(s: &str) -> String {
    // FNV-1a — deterministic, no dep, fine for an id suffix.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ijima_core::harness::Harness;

    fn ctx() -> MiningContext {
        MiningContext {
            session_id: "sess_1".into(),
            project: "ijima".into(),
            harness: Harness::Pi,
            now: "0".into(),
        }
    }

    #[test]
    fn extracts_we_decided_clause() {
        let got = extract_decisions("We decided to use SurrealDB for storage.", &ctx());
        assert_eq!(got.len(), 1);
        match &got[0] {
            Extraction::Auto(m) => {
                assert!(m.content.contains("use SurrealDB for storage"));
                assert_eq!(m.topic, "decisions");
                assert_eq!(m.source, MemorySource::Mined);
                assert_eq!(m.session_id.as_deref(), Some("sess_1"));
            }
            other => panic!("expected Auto, got {other:?}"),
        }
    }

    #[test]
    fn extracts_lets_go_with() {
        let got = extract_decisions("Let's go with the brute-force cosine search.", &ctx());
        assert_eq!(got.len(), 1);
        match &got[0] {
            Extraction::Auto(m) => assert!(m.content.contains("brute-force cosine search")),
            other => panic!("expected Auto, got {other:?}"),
        }
    }

    #[test]
    fn ignores_substring_in_other_words() {
        // "redecided" should not match "decided to" at a non-boundary.
        let got = extract_decisions("we redecided to nothing here", &ctx());
        // "we redecided to nothing" — "decided to" is a substring of
        // "redecided to" but the left boundary check blocks it. However the
        // leading "we " is followed by "redecided", and there is no bare
        // "decided to", so this yields nothing.
        assert!(got.is_empty(), "got {got:?}");
    }

    #[test]
    fn extracts_https_url() {
        let got = extract_references("see https://example.com/foo for details.", &ctx());
        assert_eq!(got.len(), 1);
        match &got[0] {
            Extraction::Auto(m) => {
                assert!(m.content.contains("https://example.com/foo"));
                assert_eq!(m.topic, "references");
            }
            other => panic!("expected Auto, got {other:?}"),
        }
    }

    #[test]
    fn extracts_multiple_urls_and_trims_punct() {
        let got = extract_references("links: https://a.com, www.b.org).", &ctx());
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn no_signal_yields_nothing() {
        let got = extract_turn("just a normal sentence with nothing to mine", &ctx());
        assert!(got.iter().all(|e| matches!(e, Extraction::Nothing)) || got.is_empty());
    }

    #[test]
    fn extract_turn_combines_decision_and_reference() {
        let got = extract_turn(
            "We decided to use candle. See https://hf.co for the loader.",
            &ctx(),
        );
        // 1 decision + 1 reference = 2
        assert_eq!(got.len(), 2);
    }
}
