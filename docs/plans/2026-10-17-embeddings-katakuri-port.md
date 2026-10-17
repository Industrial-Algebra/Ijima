# Ijima — `embeddings-katakuri` port (Katakuri S1–S4 adoption)

**Status:** contract for generation dispatch (ia-moment AUTHOR step).
**Branch:** `feature/embeddings-katakuri` off develop `7943514`.
**Dependency:** Katakuri `b7a3a3f` (S1–S4 ladder merged; git rev pinned).

## 1. Goal

Second embedding backend, parallel to `embeddings-candle`, with identical
public shape and **bit-identical embeddings**: same candle forward pass
(same weights, same config semantics, same token ids), but every file
acquisition, validation, and tokenization routed through Katakuri —
dropping `hf-hub` and the HF `tokenizers` crate from the new path.

`embeddings-candle` stays untouched (blast-radius containment — the
pattern that kept default builds safe during the yank). Both features may
be enabled together (the parity test requires it); when both are on,
**katakuri wins** in the server wiring so a both-features build exercises
the new path.

## 2. Port sources (binding)

- Consumer being ported: `ijima-server/src/embeddings_candle.rs` (read it
  first — module docs, `from_hub`/`from_env`/`from_hub_model`, `Embedder`
  impl, masked mean-pool + L2-normalize math, error mapping style).
- Port plans: Katakuri `docs/plans/2026-10-17-katakuri-s3-model-specs.md`
  §8 and `docs/plans/2026-10-17-katakuri-s4-hub.md` §8 (the addenda this
  contract executes).
- Server wiring seam: `ijima-server/src/server.rs` ~L148–170 (the
  `#[cfg(feature = "embeddings-candle")]` embedder block) — extend the
  chain per §5.

## 3. Cargo.toml (`ijima-server`)

Feature (next to `embeddings-candle`, same comment style):

```toml
# Katakuri-backed embedding backend: same candle forward pass as
# `embeddings-candle`, but file acquisition + validation + tokenization
# through Katakuri (S1–S4). Drops hf-hub and the HF tokenizers crate
# from this path. Takes precedence when both embedding features are on.
embeddings-katakuri = ["dep:candle-core", "dep:candle-nn", "dep:candle-transformers", "dep:katakuri", "ijima-core/serde"]
```

Dependency (in the candle group, ledger comment):

```toml
# Katakuri: the IA model-ecosystem layer (safetensors validation,
# WordPiece tokenizer with golden-parity to tokenizers 0.21, BERT
# specs, hub fetch + content-addressed cache). Git-pinned to the
# S1–S4 ladder merge; crates.io publish pending (Katakuri #LADDER).
katakuri = { git = "https://github.com/Industrial-Algebra/Katakuri", rev = "b7a3a3f", features = ["hub-net"], optional = true }
```

(`hub-net` implies `hub`; tokenizer/model/safetensors are default
modules. No other Cargo.toml changes. Do NOT touch the
embeddings-candle deps.)

## 4. New file `ijima-server/src/embeddings_katakuri.rs`

Mirror `embeddings_candle.rs` structure and doc style exactly (module
docs updated to describe the Katakuri path). Public surface:

```rust
pub const DEFAULT_MODEL: &str = "sentence-transformers/all-MiniLM-L6-v2";

pub struct KatakuriEmbedder { /* model: BertModel, tokenizer: Mutex<katakuri::tokenizer::Tokenizer>,
                                 device: Device, model_id: String */ }

impl KatakuriEmbedder {
    pub fn from_hub() -> Result<Self>                       // DEFAULT_MODEL @ main
    pub fn from_env() -> Result<Self>                       // IJIMA_EMBED_MODEL / IJIMA_EMBED_REVISION
    pub fn from_hub_model(model_id: &str, revision: &str) -> Result<Self>
}
```

`load(model_id, revision)`:

1. `let fetcher = Arc::new(katakuri::hub::UreqFetcher::new())` — with
   token when `HF_TOKEN` is set: `UreqFetcher::with_token(std::env::var("HF_TOKEN")...)`.
2. `let hub = katakuri::hub::Hub::with_default_root(fetcher)?` (maps to
   `IjimaError::Store { detail }`).
3. `RepoId::parse(model_id)?`, `Revision::new(revision)?` — same env
   override semantics as `embeddings_candle::from_env` (`main` default).
4. Three `hub.get(&repo, &rev, name)?` for `config.json`,
   `tokenizer.json`, `model.safetensors` → local paths.
5. Read + validate through Katakuri:
   - `let config = std::fs::read_to_string(config_path)?;`
     `let spec = katakuri::model::BertSpec::from_config_json(&config)?`
   - `let weights = std::fs::read(weights_path)?;`
     `let st = katakuri::safetensors::SafeTensors::parse(&weights)?;`
     `spec.validate_weights(&st)?`
   - `let tk_bytes = std::fs::read_to_string(tokenizer_path)?;` — read as
     bytes; `let tokenizer = katakuri::tokenizer::Tokenizer::from_tokenizer_json(&bytes)?;`
   - `spec.check_tokenizer_compatibility(&tokenizer)?`
6. candle `Config` built **field-by-field from the spec accessors**:
   `hidden_size, num_attention_heads, num_hidden_layers,
   intermediate_size, vocab_size, type_vocab_size,
   max_position_embeddings, layer_norm_eps, pad_token_id` — and
   **`hidden_act = HiddenAct::GeluApproximate` unconditionally PINNED**:
   the reference consumer overrides the config's `gelu` to the tanh
   approximation regardless of what config.json says (pi-mempalace
   migration parity). Copy that behavior bit-for-bit; changing it is a
   deliberate embedding-semantics decision, NOT part of this port.
   (candle `Config` field list: read the vendored
   `candle-transformers-0.11/src/models/bert.rs` and populate every
   field; defaults for the rest.)
7. Weights into candle: prefer
   `VarBuilder::from_buffered_safetensors(&weights, DTYPE, &device)` (the
   90 MB bytes are already in memory from validation — single read, no
   `unsafe`, no second mmap). If candle 0.11 lacks it, fall back to
   `VarBuilder::from_mmaped_safetensors(&[weights_path], DTYPE, &device)`
   with the same SAFETY comment shape as `embeddings_candle.rs`; record
   which path shipped.
8. `BertModel::load(vb, &config)?` — unchanged compute.

`impl Embedder for KatakuriEmbedder` — **copy the `embed()` math
verbatim** from `embeddings_candle.rs` (tokenize → three tensors →
forward → masked mean pool → L2 normalize → `Vec<f32>`), substituting
only the tokenizer call:

```rust
let encoding = { let t = self.tokenizer.lock().expect("tokenizer poisoned");
                 t.encode(text, true) };          // katakuri S2 — infallible
let input_ids = encoding.ids.clone();              // Vec<u32>
let attention_mask = encoding.attention_mask.clone();
let token_type_ids = vec![0u32; input_ids.len()];
// ... identical tensor + pooling + normalize code ...
```

`dim()`/`model_id()` same semantics. Error mapping: one helper per error
source (`katakuri_err(e: katakuri::error::KatakuriError) -> IjimaError`
→ `Store { detail: "katakuri: {e}" }`), reusing the existing
`candle_err`/`io_err` patterns.

Tests (in-file, mirroring `embeddings_candle.rs`'s test module):
- `reports_default_dim` — trivial, no model.
- `#[tokio::test] #[ignore] embeds_real_sentences` — same semantic
  similarity assertions as the candle one (dog/park vs WebAssembly,
  magnitude ~1). Doc-comment the run command
  (`IJIMA_EMBED_*`-free; downloads via the katakuri hub on first run,
  `~/.cache/katakuri/hub` thereafter).

## 5. `server.rs` wiring (the precedence chain)

Replace the two `#[cfg]` blocks at ~L148–170 with a three-way chain
(keep bodies otherwise identical — env/config resolution, tracing line,
`open_persistent_with`):

```rust
#[cfg(feature = "embeddings-katakuri")]
let embedder: Option<Arc<dyn ijima_core::Embedder>> = { /* same shape,
    module crate::embeddings_katakuri, DEFAULT_MODEL from it */ };
#[cfg(all(feature = "embeddings-candle", not(feature = "embeddings-katakuri")))]
let embedder: Option<Arc<dyn ijima_core::Embedder>> = { /* existing body */ };
#[cfg(not(any(feature = "embeddings-candle", feature = "embeddings-katakuri")))]
let embedder: Option<Arc<dyn ijima_core::Embedder>> = None;
```

And the same three-way split for the `store_inner` block below it
(`open_persistent_with` under katakuri OR candle, plain otherwise).
`lib.rs`: add `pub mod embeddings_katakuri;` under its cfg + extend the
feature-bullet doc comment (one line, after the `embeddings-candle`
bullet).

## 6. Parity acceptance test (the port's definition of done)

`ijima-server/tests/embedding_parity.rs` —
`#[cfg(all(feature = "embeddings-candle", feature = "embeddings-katakuri"))]`,
`#[ignore]`, env-gated on `KATAKURI_TEST_PARITY=1` (needs network on
first run; both backends cache locally afterward):

1. Build `CandleEmbedder::from_hub()` and `KatakuriEmbedder::from_hub()`
   (same default model + revision).
2. Embed a fixed set of ~8 sentences through both (include the
   S2-golden-adjacent edge shapes: trailing punctuation, a long sentence,
   a one-word sentence, mixed case, a Greek-final-sigma sentence
   `&q: ΟΔΟΣ`). Hmm — the sigma case: katakuri lowercases per-char;
   the HF `tokenizers` crate path does the same; both produce the same
   ids. Include it: it pins the per-char parity that S2's review
   established.
3. Assert **exact bit-equality** per dimension
   (`a.0 == b.0` element-wise; both are deterministic CPU f32 through
   the same candle kernels with the same weights and identical token
   ids — anything else is a port defect). If a platform ever diverges
   at the last ulp, relax to `max abs diff <= 1e-6` ONLY with a written
   justification in the test comment; start strict.
4. Also assert both `model_id()`s are equal.

## 7. Gates (this repo's conventions + the new feature)

```
cargo check -p ijima-server                                  # default, no embedding deps
cargo check -p ijima-server --features embeddings-katakuri
cargo check -p ijima-server --features embeddings-candle      # untouched, still builds
cargo test  -p ijima-server --features embeddings-katakuri    # lib tests incl. reports_default_dim
cargo clippy -p ijima-server --features embeddings-katakuri --all-targets -- -D warnings
cargo clippy --workspace --all-targets -- -D warnings          # repo-standard gate
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc -p ijima-server --no-deps
RUSTDOCFLAGS="-D warnings" cargo doc -p ijima-server --no-deps --features embeddings-katakuri
# acceptance (this session, network on first run):
KATAKURI_TEST_PARITY=1 cargo test -p ijima-server --features 'embeddings-candle embeddings-katakuri' --test embedding_parity -- --ignored
```

Note: the katakuri git dep lands in the workspace `Cargo.lock` — commit
it. Do NOT touch any other workspace crate.

## 8. Ambiguity protocol

Mirror the consumer, not the ideal: anywhere the candle module made a
choice (error strings, doc tone, test style), copy it. If candle 0.11's
`VarBuilder` lacks `from_buffered_safetensors`, use the mmap path and
record it. If the exact-parity assertion fails on the first run,
INVESTIGATE before relaxing — divergence means the port changed
tokenization, config, or weights somewhere, and that is a defect, not
noise.
