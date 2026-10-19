// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! Katakuri-backed candle embedding backend.
//!
//! The Katakuri adoption of the IA-standard candle embedding path:
//! identical candle forward pass (same weights, same config semantics,
//! same token ids), but every file acquisition, validation, and
//! tokenization step is routed through [Katakuri] — no `hf-hub`, no HF
//! `tokenizers` crate. Loads a sentence-transformer model (default:
//! `all-MiniLM-L6-v2`, 384-dim, for pi-mempalace migration parity) and
//! implements [`ijima_core::Embedder`].
//!
//! ## Model loading
//!
//! On construction, model files are fetched from the Hugging Face Hub by
//! Katakuri's `Hub` (content-addressed cache under
//! `~/.cache/katakuri/hub`) and validated before the candle `BertModel`
//! sees them: `BertSpec` checks the config, `SafeTensors` checks the
//! weight layout, and `Tokenizer` reproduces the reference tokenization.
//! The first load downloads ~90 MB; subsequent loads are instant from
//! cache.
//!
//! Weights reach candle through `VarBuilder::from_buffered_safetensors`:
//! the bytes are already in memory from validation, so there is a single
//! read, no second mmap, and no `unsafe`.
//!
//! [Katakuri]: https://github.com/Industrial-Algebra/Katakuri

use std::sync::{Arc, Mutex};

use candle_core::{Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config, DTYPE, HiddenAct};
use ijima_core::embeddings::{DEFAULT_EMBEDDING_DIM, Embedder, Embedding};
use ijima_core::{IjimaError, Result};

/// The default Hugging Face model id for Ijima embeddings.
pub const DEFAULT_MODEL: &str = "sentence-transformers/all-MiniLM-L6-v2";

/// A Katakuri-backed sentence embedder.
///
/// Construct with [`KatakuriEmbedder::from_hub`] (downloads the model) or
/// [`KatakuriEmbedder::from_hub_model`] (custom model id). Implements
/// [`Embedder`] — `embed()` tokenizes (Katakuri WordPiece), runs a BERT
/// forward pass, mean-pools (attention-masked), and L2-normalizes to
/// produce a 384-dim vector.
pub struct KatakuriEmbedder {
    model: BertModel,
    tokenizer: Mutex<katakuri::tokenizer::Tokenizer>,
    device: Device,
    /// The HF model id + revision this embedder loaded (provenance, D10).
    model_id: String,
}

impl KatakuriEmbedder {
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
        // `HF_TOKEN`, when set, authenticates the hub fetch (gated repos,
        // higher rate limits); otherwise the fetch is anonymous.
        let fetcher = match std::env::var("HF_TOKEN") {
            Ok(token) => Arc::new(katakuri::hub::UreqFetcher::with_token(token)),
            Err(_) => Arc::new(katakuri::hub::UreqFetcher::new()),
        };
        let hub = katakuri::hub::Hub::with_default_root(fetcher).map_err(katakuri_err)?;
        let repo = katakuri::hub::RepoId::parse(model_id).map_err(katakuri_err)?;
        let rev = katakuri::hub::Revision::new(revision).map_err(katakuri_err)?;

        let config_path = hub.get(&repo, &rev, "config.json").map_err(katakuri_err)?;
        let tokenizer_path = hub
            .get(&repo, &rev, "tokenizer.json")
            .map_err(katakuri_err)?;
        let weights_path = hub
            .get(&repo, &rev, "model.safetensors")
            .map_err(katakuri_err)?;

        let config_json = std::fs::read_to_string(&config_path).map_err(io_err)?;
        let spec =
            katakuri::model::BertSpec::from_config_json(&config_json).map_err(katakuri_err)?;

        let weights = std::fs::read(&weights_path).map_err(io_err)?;
        {
            let st = katakuri::safetensors::SafeTensors::parse(&weights).map_err(katakuri_err)?;
            spec.validate_weights(&st).map_err(katakuri_err)?;
        }

        let tokenizer_bytes = std::fs::read(&tokenizer_path).map_err(io_err)?;
        let tokenizer = katakuri::tokenizer::Tokenizer::from_tokenizer_json(&tokenizer_bytes)
            .map_err(katakuri_err)?;
        spec.check_tokenizer_compatibility(&tokenizer)
            .map_err(katakuri_err)?;

        // candle `Config`, populated field-by-field from the validated
        // spec; the rest keep candle's defaults (training-only fields the
        // forward pass never reads). `hidden_act` is PINNED to the tanh
        // approximation: the candle backend overrides config.json's
        // `gelu` the same way for pi-mempalace migration parity, so the
        // two backends compute identical embeddings. Changing this is a
        // deliberate embedding-semantics decision, not a refactor.
        let config = Config {
            vocab_size: spec.vocab_size(),
            hidden_size: spec.hidden_size(),
            num_hidden_layers: spec.num_hidden_layers(),
            num_attention_heads: spec.num_attention_heads(),
            intermediate_size: spec.intermediate_size(),
            hidden_act: HiddenAct::GeluApproximate,
            max_position_embeddings: spec.max_position_embeddings(),
            type_vocab_size: spec.type_vocab_size(),
            layer_norm_eps: spec.layer_norm_eps(),
            pad_token_id: spec.pad_token_id() as usize,
            ..Default::default()
        };

        // The ~90 MB weight bytes are already resident from validation —
        // hand them to candle directly (single read, no `unsafe`, no
        // second mmap).
        let vb =
            VarBuilder::from_buffered_safetensors(weights, DTYPE, &device).map_err(candle_err)?;
        let model = BertModel::load(vb, &config).map_err(candle_err)?;

        Ok(Self {
            model,
            tokenizer: Mutex::new(tokenizer),
            device,
            model_id: format!("{model_id}@{revision}"),
        })
    }
}

impl Embedder for KatakuriEmbedder {
    fn dim(&self) -> usize {
        DEFAULT_EMBEDDING_DIM
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn embed(&self, text: &str) -> Result<Embedding> {
        // Katakuri's `encode` is infallible (unknown pieces map to
        // `[UNK]` by design), so there is no error to map here.
        let encoding = {
            let tok = self.tokenizer.lock().expect("tokenizer poisoned");
            tok.encode(text, true)
        };

        let input_ids = encoding.ids.clone();
        let attention_mask = encoding.attention_mask.clone();
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

fn katakuri_err(e: katakuri::error::KatakuriError) -> IjimaError {
    IjimaError::Store {
        detail: format!("katakuri: {e}"),
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
    /// download (first run; cached under `~/.cache/katakuri/hub`
    /// thereafter). Run manually:
    ///   `cargo test --features embeddings-katakuri -- --ignored katakuri`
    #[tokio::test]
    #[ignore]
    async fn embeds_real_sentences() {
        let embedder = KatakuriEmbedder::from_hub().expect("load model");

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
