// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! # ijima-core
//!
//! Core schema, stores, error types, and API contract for the
//! [Ijima](https://github.com/Industrial-Algebra/Ijima) centralized
//! agentic memory backend.
//!
//! Ijima is the single source of truth for agentic memory across the IA
//! ecosystem. This crate holds the type-level contract every other Ijima
//! crate and every harness adapter speaks: the memory palace model, the
//! knowledge graph, the session context repository, and the unified error
//! type.
//!
//! The store implementations (SQLite + vector index), the HTTP server,
//! the mining engine, and the harness client live in their own crates
//! (`ijima-server`, `ijima-miner`, `ijima-client`). This crate deliberately
//! stays transport- and backend-free so the contract can be shared without
//! pulling in heavy I/O dependencies.
//!
//! ## Features
//!
//! - `std` (default): Standard library support. Without it, only the
//!   pure type definitions compile.
//!
//! - `embeddings`: pure `Embedder` trait + `Embedding` vector type. The
//!   default dimension is 384 to match pi-mempalace's `all-MiniLM-L6-v2`
//!   for migration parity.
//! - `serde`: Serialize/Deserialize impls for all domain types. Required
//!   by the store backends and the HTTP API.
//! - `store`: the pure async `Store` trait every backend implements.
//!
//! ## Status
//!
//! Scaffolded — the API surface is being filled in test-first. See
//! `docs/HANDOFF.md` and `docs/DESIGN.md` at the repository root.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod capabilities;
pub mod embeddings;
pub mod error;
pub mod harness;
pub mod knowledge;
pub mod memory;
pub mod namespace;
pub mod session;
pub mod store;

pub use embeddings::{DEFAULT_EMBEDDING_DIM, Embedder, Embedding};
pub use error::IjimaError;
pub use knowledge::{Entity, EntityId, EntityRecord, KgStats, KnowledgeGraph, Triple};
pub use memory::{Memory, MemoryId, MemorySource};
pub use namespace::{Namespace, NamespaceId, NamespaceKind};
pub use session::{Session, SessionId, SessionTurn, TurnRole};
pub use store::Store;

/// Convenience `Result` alias used throughout the Ijima crates.
pub type Result<T, E = IjimaError> = core::result::Result<T, E>;
