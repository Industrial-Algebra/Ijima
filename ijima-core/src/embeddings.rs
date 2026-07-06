// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! Embedding contract — pure trait, no backend.
//!
//! The trait lives in `ijima-core` so the store, miner, and client can
//! depend on it without pulling in a heavy ML backend. Concrete
//! implementations (candle, remote API, ...) live in `ijima-server`
//! behind feature gates.
//!
//! ## Default dimensionality
//!
//! [`DEFAULT_EMBEDDING_DIM`] is 384 to match pi-mempalace's
//! `all-MiniLM-L6-v2`, so the live `memories.db` corpus migrates without
//! re-embedding. Configurable dimensions are a future concern (requires a
//! re-embed pass on dimension change).

use crate::Result;

/// Default embedding dimensionality — 384, matching pi-mempalace's
/// `all-MiniLM-L6-v2` for migration parity.
pub const DEFAULT_EMBEDDING_DIM: usize = 384;

/// A dense embedding vector for a piece of text.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct Embedding(pub Vec<f32>);

impl Embedding {
    /// Returns the dimensionality of the vector.
    pub fn dim(&self) -> usize {
        self.0.len()
    }

    /// Returns the underlying slice.
    pub fn as_slice(&self) -> &[f32] {
        &self.0
    }
}

/// The embedding contract every backend implements.
///
/// Backends ship in `ijima-server` (candle is the IA-standard default,
/// consistent with Quantizon); a remote-API backend may follow.
///
/// `Send + Sync` so an `Arc<dyn Embedder>` can be shared across an
/// async store's worker pool.
pub trait Embedder: Send + Sync {
    /// Dimensionality of the vectors this embedder produces.
    fn dim(&self) -> usize;

    /// Embeds a single piece of text.
    ///
    /// # Errors
    ///
    /// Returns [`crate::IjimaError::Store`] on a backend failure (model
    /// load, inference, device error).
    fn embed(&self, text: &str) -> Result<Embedding>;

    /// Embeds a batch of texts. The default loops over [`embed`];
    /// backends with batch acceleration override this.
    ///
    /// # Errors
    ///
    /// Propagates any per-item embedding error.
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Embedding>> {
        texts.iter().map(|t| self.embed(t)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic toy embedder used only to exercise the trait
    /// contract in core. Real backends live in `ijima-server`.
    struct ConstEmbedder;
    impl Embedder for ConstEmbedder {
        fn dim(&self) -> usize {
            2
        }
        fn embed(&self, text: &str) -> Result<Embedding> {
            Ok(Embedding(vec![text.len() as f32, 0.0]))
        }
    }

    #[test]
    fn default_dim_matches_mempalace_for_migration() {
        assert_eq!(DEFAULT_EMBEDDING_DIM, 384);
    }

    #[test]
    fn embed_batch_defaults_to_per_item_loop() {
        let e = ConstEmbedder;
        let got = e.embed_batch(&["a", "bb", "ccc"]).expect("must embed");
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].dim(), 2);
        assert_eq!(got[2].as_slice(), &[3.0, 0.0]);
    }
}
