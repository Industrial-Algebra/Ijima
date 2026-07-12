// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! Candle (Hugging Face Rust ML) embedding backend.
//!
//! The IA-standard embedding backend, consistent with Quantizon. Loads a
//! sentence-transformer model (default: `all-MiniLM-L6-v2`, 384-dim, for
//! pi-mempalace migration parity) and implements [`ijima_core::Embedder`].
//!
//! ## Model loading
//!
//! On construction, model files are fetched from the Hugging Face Hub
//! (cached under `~/.cache/huggingface/hub` by `hf-hub`) and the weights
//! are memory-mapped via `VarBuilder::from_mmaped_safetensors`. The first
//! load downloads ~90 MB; subsequent loads are instant from cache.
//!
//! `unsafe` is required only for the memory-mapping call (a candle API
//! that mmaps a trusted model file) and is scoped to this module.

#![allow(unsafe_code)]

use std::sync::Mutex;

use candle_core::{Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config, DTYPE, HiddenAct};
use hf_hub::{HFClientSync, HFError, split_id};
use ijima_core::embeddings::{DEFAULT_EMBEDDING_DIM, Embedder, Embedding};
use ijima_core::{IjimaError, Result};
use tokenizers::{PaddingParams, PaddingStrategy, Tokenizer};

/// The default Hugging Face model id for Ijima embeddings.
pub const DEFAULT_MODEL: &str = "sentence-transformers/all-MiniLM-L6-v2";

/// A candle-backed sentence embedder.
///
/// Construct with [`CandleEmbedder::from_hub`] (downloads the model) or
/// [`CandleEmbedder::from_hub_model`] (custom model id). Implements
/// [`Embedder`] — `embed()` tokenizes, runs a BERT forward pass, mean-
/// pools (attention-masked), and L2-normalizes to produce a 384-dim
/// vector.
pub struct CandleEmbedder {
    model: BertModel,
    tokenizer: Mutex<Tokenizer>,
    device: Device,
    /// The HF model id + revision this embedder loaded (provenance, D10).
    model_id: String,
}

impl CandleEmbedder {
    /// Loads the default model (`all-MiniLM-L6-v2`) from the HF Hub
    /// onto the CPU.
    ///
    /// # Errors
    ///
    /// Returns [`IjimaError::Store`] if the model files cannot be
    /// downloaded or the weights/config/tokenizer fail to load.
    pub fn from_hub() -> Result<Self> {
        Self::from_hub_model(DEFAULT_MODEL, "main")
    }

    /// Loads the model selected by `IJIMA_EMBED_MODEL` /
    /// `IJIMA_EMBED_REVISION` (defaulting to MiniLM-L6-v2 @ main).
    /// This is the config-driven entry point for near-term model
    /// iteration (D10) — swapping the embedding model is a config
    /// change, not a code change.
    ///
    /// # Errors
    ///
    /// Returns [`IjimaError::Store`] on download or load failure.
    pub fn from_env() -> Result<Self> {
        let model =
            std::env::var("IJIMA_EMBED_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());
        let revision = std::env::var("IJIMA_EMBED_REVISION").unwrap_or_else(|_| "main".to_string());
        Self::from_hub_model(&model, &revision)
    }

    /// Loads an arbitrary sentence-transformer BERT model from the HF
    /// Hub onto the CPU.
    ///
    /// # Errors
    ///
    /// Returns [`IjimaError::Store`] on download or load failure.
    pub fn from_hub_model(model_id: &str, revision: &str) -> Result<Self> {
        Self::load(model_id, revision, Device::Cpu)
    }

    fn load(model_id: &str, revision: &str, device: Device) -> Result<Self> {
        let (owner, model_name) = split_id(model_id);
        let client = HFClientSync::new().map_err(hub_err)?;
        let repo = client.model(owner, model_name);
        let rev = Some(revision.to_string());

        let config_filename = repo
            .download_file()
            .filename("config.json")
            .maybe_revision(rev.clone())
            .send()
            .map_err(hub_err)?;
        let tokenizer_filename = repo
            .download_file()
            .filename("tokenizer.json")
            .maybe_revision(rev.clone())
            .send()
            .map_err(hub_err)?;
        let weights_filename = repo
            .download_file()
            .filename("model.safetensors")
            .maybe_revision(rev)
            .send()
            .map_err(hub_err)?;

        let config_str = std::fs::read_to_string(&config_filename).map_err(io_err)?;
        let mut config: Config =
            serde_json::from_str(&config_str).map_err(|e| IjimaError::Store {
                detail: format!("config parse: {e}"),
            })?;
        config.hidden_act = HiddenAct::GeluApproximate;

        let mut tokenizer =
            Tokenizer::from_file(&tokenizer_filename).map_err(|e| IjimaError::Store {
                detail: format!("tokenizer load: {e}"),
            })?;
        tokenizer.with_padding(Some(PaddingParams {
            strategy: PaddingStrategy::BatchLongest,
            ..Default::default()
        }));

        // SAFETY: `from_mmaped_safetensors` mmaps the model file
        // downloaded from the (trusted) HF Hub. The file is read-only
        // and its lifetime is bounded by the returned VarBuilder, which
        // is consumed by BertModel::load before this function returns.
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&[weights_filename], DTYPE, &device)
                .map_err(candle_err)?
        };
        let model = BertModel::load(vb, &config).map_err(candle_err)?;

        Ok(Self {
            model,
            tokenizer: Mutex::new(tokenizer),
            device,
            model_id: format!("{model_id}@{revision}"),
        })
    }
}

impl Embedder for CandleEmbedder {
    fn dim(&self) -> usize {
        DEFAULT_EMBEDDING_DIM
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn embed(&self, text: &str) -> Result<Embedding> {
        let encoding = {
            let tok = self.tokenizer.lock().expect("tokenizer poisoned");
            tok.encode(text, true).map_err(|e| IjimaError::Store {
                detail: format!("tokenize: {e}"),
            })?
        };

        let input_ids = encoding.get_ids().to_vec();
        let attention_mask = encoding.get_attention_mask().to_vec();
        let token_type_ids = vec![0u32; input_ids.len()];

        let token_ids = Tensor::new(input_ids.as_slice(), &self.device)
            .map_err(candle_err)?
            .unsqueeze(0)
            .map_err(candle_err)?;
        let attention_mask = Tensor::new(attention_mask.as_slice(), &self.device)
            .map_err(candle_err)?
            .unsqueeze(0)
            .map_err(candle_err)?;
        let token_type_ids = Tensor::new(token_type_ids.as_slice(), &self.device)
            .map_err(candle_err)?
            .unsqueeze(0)
            .map_err(candle_err)?;

        let embeddings = self
            .model
            .forward(&token_ids, &token_type_ids, Some(&attention_mask))
            .map_err(candle_err)?;

        // Attention-masked mean pooling over the token dimension.
        let mask = attention_mask
            .to_dtype(DTYPE)
            .map_err(candle_err)?
            .unsqueeze(2)
            .map_err(candle_err)?;
        let sum_mask = mask.sum(1).map_err(candle_err)?;
        let masked_sum = (embeddings.broadcast_mul(&mask))
            .map_err(candle_err)?
            .sum(1)
            .map_err(candle_err)?;
        let pooled = masked_sum.broadcast_div(&sum_mask).map_err(candle_err)?;

        // L2 normalize.
        let norm = pooled
            .sqr()
            .map_err(candle_err)?
            .sum_all()
            .map_err(candle_err)?;
        let norm = norm.sqrt().map_err(candle_err)?;
        let normalized = pooled.broadcast_div(&norm).map_err(candle_err)?;

        // Extract to Vec<f32> — shape is [1, 384].
        let vec = normalized
            .squeeze(0)
            .map_err(candle_err)?
            .to_vec1::<f32>()
            .map_err(candle_err)?;
        Ok(Embedding(vec))
    }
}

fn candle_err(e: candle_core::Error) -> IjimaError {
    IjimaError::Store {
        detail: format!("candle: {e}"),
    }
}

fn hub_err(e: HFError) -> IjimaError {
    IjimaError::Store {
        detail: format!("hf-hub: {e}"),
    }
}

fn io_err(e: std::io::Error) -> IjimaError {
    IjimaError::Store {
        detail: format!("io: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_default_dim() {
        // dim() doesn't need the model loaded.
        let dim = DEFAULT_EMBEDDING_DIM;
        assert_eq!(dim, 384);
    }

    /// End-to-end: downloads all-MiniLM-L6-v2, embeds two sentences,
    /// asserts dimensionality and that a semantically-similar pair has
    /// higher cosine similarity than a dissimilar pair.
    ///
    /// `#[ignore]` because it requires network access + a ~90 MB model
    /// download. Run manually:
    ///   `cargo test --features embeddings-candle -- --ignored candle`
    #[tokio::test]
    #[ignore]
    async fn embeds_real_sentences() {
        let embedder = CandleEmbedder::from_hub().expect("load model");

        let a = embedder.embed("A dog plays in the park").expect("embed a");
        let b = embedder
            .embed("A puppy runs in the garden")
            .expect("embed b");
        let c = embedder
            .embed("Rust compiles WebAssembly")
            .expect("embed c");

        assert_eq!(a.dim(), 384);
        assert_eq!(b.dim(), 384);
        assert_eq!(c.dim(), 384);

        let sim_ab = cosine(&a, &b);
        let sim_ac = cosine(&a, &c);
        // Similar sentences should score higher than dissimilar ones.
        assert!(
            sim_ab > sim_ac,
            "sim(a,b)={sim_ab:.3} should exceed sim(a,c)={sim_ac:.3}"
        );
        // L2-normalized embeddings have magnitude ~1.
        let mag = a.0.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((mag - 1.0).abs() < 1e-3, "magnitude {mag} != 1.0");
    }

    fn cosine(a: &Embedding, b: &Embedding) -> f32 {
        a.0.iter().zip(&b.0).map(|(x, y)| x * y).sum::<f32>()
    }
}
