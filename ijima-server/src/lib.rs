// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! # ijima-server
//!
//! HTTP daemon and store backends for the
//! [Ijima](https://github.com/Industrial-Algebra/Ijima) centralized
//! agentic memory backend.
//!
//! This crate is the Rust successor to the pi-mempalace fork's
//! `pi-mempalace-server.ts` remote backend. It owns the store backends
//! (SurrealDB primary — see `docs/DESIGN.md` D6), the embedding/vector
//! index, the JSON-RPC-style HTTP surface that harnesses speak, and the
//! systemd-friendly daemon entry point.
//!
//! The transport-free type contract — including the [`ijima_core::Store`]
//! trait this crate implements — lives in [`ijima_core`]; the extraction
//! engine lives in [`ijima_miner`]; harness adapters live in
//! [`ijima_client`].
//!
//! ## Features
//!
//! - `std` (default): Standard library support.
//! - `http` (default): axum-based HTTP/JSON server + the `ijima` binary.
//!   Disable to embed the store as a library without spawning a server.
//! - `backend-surreal`: SurrealDB store backend (primary). Embedded
//!   `kv-mem` engine; server mode is a future addition.
//! - `backend-sqlite`: SQLite store backend — **migration only**. Reads
//!   the live pi-mempalace corpus once during import.
//! - `embeddings-candle`: candle (Hugging Face Rust ML) embedding backend.
//!   The IA-standard default, consistent with Quantizon. Builds candle
//!   on CPU; GPU/CUDA acceleration is a future opt-in.
//! - `server-auth`: Schubert proof-carrying capability tokens for both
//!   authentication and authorization on Gr(4,8). Loads
//!   [`policy/policy.toml`](https://github.com/Industrial-Algebra/Ijima).
//!   When off, the store runs unauthenticated (embedded/tests).
//!
//! ## Status
//!
//! Scaffolded — store backends, schema import, and HTTP routes are
//! filled in test-first. See `docs/HANDOFF.md` and `docs/DESIGN.md`.

#![deny(unsafe_code)]
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "backend-surreal")]
pub mod backend_surreal;

#[cfg(feature = "backend-surreal")]
pub use backend_surreal::SurrealStore;

#[cfg(feature = "embeddings-candle")]
pub mod embeddings_candle;

#[cfg(feature = "server-auth")]
pub mod auth;

#[cfg(feature = "server-auth")]
pub mod key_store;

#[cfg(feature = "server-auth")]
pub use auth::{AuthenticatedPrincipal, IjimaAuth};

#[cfg(feature = "http")]
pub mod api;

#[cfg(all(feature = "http", feature = "server-auth"))]
pub mod extractor;

/// Redaction/scrub filter for the personal → shared promotion boundary (D9 §2).
pub mod redaction;

/// Doctrine seed-pack format parser + ingest client (D9).
pub mod doctrine;

#[cfg(feature = "http")]
pub mod server;
