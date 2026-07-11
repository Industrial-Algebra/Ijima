// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! Palace organization types — the navigational views over a namespace's
//! memory palace (Phase 3.1 + 3.2).
//!
//! These are pure aggregations over stored memories' `project` and `topic`
//! fields. No new storage; they let an agent *browse* the shape of its own
//! palace: which rooms (topics) exist, how projects are taxonomized, and which
//! projects connect via shared topics (tunnels).

use crate::Memory;

/// A room is a single `(project, topic)` cell with its memory count.
///
/// Mirrors pi-mempalace's "room" concept: a topic within a project.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Room {
    /// The project this room lives under.
    pub project: String,
    /// The topic (room name).
    pub topic: String,
    /// Memories in this room.
    pub count: usize,
}

/// One project's taxonomy: its rooms (topics) with counts, plus a total.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ProjectTaxon {
    /// The project name.
    pub project: String,
    /// Rooms under this project, ordered by count desc then topic.
    pub rooms: Vec<Room>,
    /// Total memories across this project's rooms.
    pub total: usize,
}

/// A topic tunnel connecting two projects.
///
/// A tunnel exists when two distinct projects both have memories tagged
/// with the same topic — the topic is the shared concern that connects
/// them (e.g. `auth`, `database`, `architecture`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Tunnel {
    /// The shared topic forming the tunnel.
    pub topic: String,
    /// First project.
    pub project_a: String,
    /// Second project.
    pub project_b: String,
    /// Memory count in `project_a` on this topic.
    pub count_a: usize,
    /// Memory count in `project_b` on this topic.
    pub count_b: usize,
}

/// The palace graph: projects as nodes, shared-topic tunnels as edges.
///
/// Powers `getPalaceGraph` — *"what connects these projects?"*
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PalaceGraph {
    /// Distinct project names (graph nodes).
    pub projects: Vec<String>,
    /// Topic tunnels between projects (graph edges).
    pub tunnels: Vec<Tunnel>,
}

/// The result of traversing a tunnel between two projects via a shared
/// topic: the actual memories from both sides, so the caller can see what
/// connects them.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TunnelTraversal {
    /// The traversed topic.
    pub topic: String,
    /// First project.
    pub project_a: String,
    /// Second project.
    pub project_b: String,
    /// Memories in `project_a` on this topic (importance-ranked).
    pub memories_a: Vec<Memory>,
    /// Memories in `project_b` on this topic (importance-ranked).
    pub memories_b: Vec<Memory>,
}
