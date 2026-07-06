// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! Centralized error type for the Ijima memory backend.

use thiserror::Error;

/// The single error enum returned by every fallible Ijima operation.
///
/// Variants follow the IA convention of carrying structured context
/// rather than opaque strings, so callers can react to specific failure
/// modes (a duplicate write, a missing entity, an unrunnable miner).
#[derive(Debug, Error)]
pub enum IjimaError {
    /// A memory write was rejected because equivalent content already
    /// exists in the palace (content-hash or semantic dedup hit).
    #[error("duplicate memory rejected: {detail}")]
    Duplicate {
        /// Human-readable description of what collided.
        detail: String,
    },

    /// A referenced entity, memory, or session was not found.
    #[error("not found: {detail}")]
    NotFound {
        /// What was missing.
        detail: String,
    },

    /// The backing store returned an error (SQLite, vector index, file I/O).
    #[error("store error: {detail}")]
    Store {
        /// Underlying store failure description.
        detail: String,
    },

    /// A harness-supplied identifier or payload failed validation.
    #[error("invalid input: {detail}")]
    InvalidInput {
        /// Why the input was rejected.
        detail: String,
    },

    /// The schema migration/import path failed.
    #[error("schema error: {detail}")]
    Schema {
        /// Migration/import failure description.
        detail: String,
    },

    /// The mining engine could not complete an extraction pass.
    #[error("mining error: {detail}")]
    Mining {
        /// Extraction failure description.
        detail: String,
    },

    /// A transport-layer failure (HTTP client/server, serialization).
    #[error("transport error: {detail}")]
    Transport {
        /// Transport failure description.
        detail: String,
    },
}

impl IjimaError {
    /// Construct a [`Duplicate`] error with the given detail string.
    pub fn duplicate(detail: impl Into<String>) -> Self {
        Self::Duplicate {
            detail: detail.into(),
        }
    }

    /// Construct a [`NotFound`] error with the given detail string.
    pub fn not_found(detail: impl Into<String>) -> Self {
        Self::NotFound {
            detail: detail.into(),
        }
    }

    /// Construct an [`InvalidInput`] error with the given detail string.
    pub fn invalid_input(detail: impl Into<String>) -> Self {
        Self::InvalidInput {
            detail: detail.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_error_carries_detail() {
        let err = IjimaError::duplicate("content hash abc123 already present");
        match err {
            IjimaError::Duplicate { detail } => {
                assert_eq!(detail, "content hash abc123 already present");
            }
            other => panic!("expected Duplicate, got {other:?}"),
        }
    }

    #[test]
    fn error_display_is_human_readable() {
        let err = IjimaError::not_found("session sess_42");
        assert_eq!(err.to_string(), "not found: session sess_42");
    }
}
