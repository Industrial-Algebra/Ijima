// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! Per-agent diary journals (Phase 3.3).
//!
//! Append-only, id-less — journals are immutable chronological logs.
//! Ordering uses a numeric `ts` field stamped by the store; the wire
//! `timestamp` field is a display string (ISO-8601).

/// One diary entry: an agent's chronological reflection.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DiaryEntry {
    /// The agent this diary belongs to (e.g. "claude", "pi").
    pub agent: String,
    /// The entry body.
    pub content: String,
    /// Optional topic tag.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub topic: Option<String>,
    /// ISO-8601 timestamp (display; ordering uses an internal numeric field).
    pub timestamp: String,
}
