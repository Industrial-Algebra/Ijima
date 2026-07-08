// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! Typed identifiers for the agentic harnesses that connect to Ijima.
//!
//! Every memory and session is tagged with its originating harness so
//! that provenance is preserved end-to-end. Using an exhaustive enum
//! rather than a raw `&str` lets the compiler flag callers when a new
//! harness is added.

/// The set of harnesses known to write to or read from Ijima.
///
/// This is intentionally exhaustive: adding a harness is a one-line
/// change that the compiler propagates to every `match` site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Harness {
    /// Dominic — meta-orchestrator.
    Dominic,
    /// Wallace — multi-user TUI.
    Wallace,
    /// Sakamoto — pipeline coding.
    Sakamoto,
    /// Tsume — gateway adapter (Discord, etc.).
    Tsume,
    /// pi — deep-work coding sessions.
    Pi,
    /// opencode — coding CLI.
    Opencode,
    /// Proserpina — critique pipeline (future).
    Proserpina,
    /// Any other harness not yet promoted to a dedicated variant.
    Other,
}

impl Harness {
    /// Returns the lowercase wire string used in stored records and the
    /// HTTP API. Stable across releases; do not rename without a
    /// migration.
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            Harness::Dominic => "dominic",
            Harness::Wallace => "wallace",
            Harness::Sakamoto => "sakamoto",
            Harness::Tsume => "tsume",
            Harness::Pi => "pi",
            Harness::Opencode => "opencode",
            Harness::Proserpina => "proserpina",
            Harness::Other => "other",
        }
    }

    /// Parses a wire string back into a [`Harness`]. Unknown strings
    /// map to [`Harness::Other`] so records from a future harness don't
    /// break deserialization.
    pub fn from_wire_str(s: &str) -> Self {
        match s {
            "dominic" => Harness::Dominic,
            "wallace" => Harness::Wallace,
            "sakamoto" => Harness::Sakamoto,
            "tsume" => Harness::Tsume,
            "pi" => Harness::Pi,
            "opencode" => Harness::Opencode,
            "proserpina" => Harness::Proserpina,
            _ => Harness::Other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_strings_are_stable_and_lowercase() {
        assert_eq!(Harness::Pi.as_wire_str(), "pi");
        assert_eq!(Harness::Tsume.as_wire_str(), "tsume");
        for h in [
            Harness::Dominic,
            Harness::Wallace,
            Harness::Sakamoto,
            Harness::Tsume,
            Harness::Pi,
            Harness::Opencode,
            Harness::Proserpina,
            Harness::Other,
        ] {
            let s = h.as_wire_str();
            assert!(
                s == s.to_ascii_lowercase(),
                "wire string for {h:?} must be lowercase"
            );
        }
    }

    #[test]
    fn from_wire_str_round_trips_known_harnesses() {
        for h in [
            Harness::Dominic,
            Harness::Wallace,
            Harness::Sakamoto,
            Harness::Tsume,
            Harness::Pi,
            Harness::Opencode,
            Harness::Proserpina,
        ] {
            assert_eq!(Harness::from_wire_str(h.as_wire_str()), h);
        }
        // Unknown strings fall back to Other (forward-compat).
        assert_eq!(Harness::from_wire_str("future-harness"), Harness::Other);
    }
}
