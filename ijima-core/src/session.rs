// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! Session-context repository domain types — the raw session transcript
//! store (HANDOFF §4). Append-only, high-fidelity; mined into the memory
//! palace by `ijima-miner`.

use crate::harness::Harness;

/// Stable opaque identifier for a session.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct SessionId(pub String);

impl SessionId {
    /// Construct a session id.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The stable wire string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The role of a session turn's author.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TurnRole {
    /// The user / operator.
    User,
    /// The agent / assistant.
    Assistant,
    /// A system message.
    System,
    /// A tool-call result.
    Tool,
}

/// A single raw turn in a session transcript.
///
/// Provenance — which harness produced this turn — is carried by the
/// enclosing session, not duplicated per turn.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SessionTurn {
    /// The session this turn belongs to.
    pub session_id: SessionId,
    /// 0-based index of this turn within the session.
    pub turn_index: u64,
    /// Author role.
    pub role: TurnRole,
    /// Raw turn content.
    pub content: String,
    /// ISO-8601 timestamp (matches pi-mempalace's TEXT timestamps).
    pub timestamp: String,
}

/// A session descriptor — the metadata for a raw transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Session {
    /// Stable opaque identifier.
    pub id: SessionId,
    /// Which harness produced this session.
    pub harness: Harness,
    /// Gateway/channel/thread identifier (e.g. a Discord thread id).
    pub channel: Option<String>,
    /// ISO-8601 start timestamp.
    pub started_at: String,
    /// ISO-8601 end timestamp, if the session has ended.
    pub ended_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_turn_round_trips() {
        let t = SessionTurn {
            session_id: SessionId::new("sess_1"),
            turn_index: 0,
            role: TurnRole::User,
            content: "hello".into(),
            timestamp: "2026-07-05T12:00:00Z".into(),
        };
        assert_eq!(t.session_id.as_str(), "sess_1");
        assert_eq!(t.role, TurnRole::User);
        assert_eq!(t.turn_index, 0);
    }
}
