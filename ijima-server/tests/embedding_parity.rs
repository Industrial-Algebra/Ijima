// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! Bit-parity acceptance for the Katakuri embedding port.
//!
//! Runs both backends through the same candle forward pass and asserts
//! exact elementwise equality of the resulting embeddings. Gated on both
//! embedding features and `KATAKURI_TEST_PARITY=1`; `#[ignore]` because
//! the first run needs network + two ~90 MB model downloads.
//!
//! Run with:
//!   `KATAKURI_TEST_PARITY=1 cargo test -p ijima-server \
//!     --features 'embeddings-candle embeddings-katakuri' \
//!     --test embedding_parity -- --ignored`

#![cfg(all(feature = "embeddings-candle", feature = "embeddings-katakuri"))]

use ijima_core::Embedder;
use ijima_server::embeddings_candle::CandleEmbedder;
use ijima_server::embeddings_katakuri::KatakuriEmbedder;

/// The fixed sentence set exercised through both backends. Covers the edge
/// shapes S2's tokenizer-parity review established: trailing punctuation,
/// a long sentence, a one-word sentence, mixed case, and a Greek
/// final-sigma word.
const SENTENCES: &[&str] = &[
    "A dog plays in the park.",
    "What is the capital of France?",
    "This is a deliberately long sentence whose only purpose is to exercise tokenization across many WordPiece splits and confirm that the two backends agree on every single token id they produce.",
    "Hello",
    "MiXeD CaSe WoRdS sHoUlD nOt ChAnGe ThE tOkEnS",
    "ΟΔΟΣ",
    "The quick brown fox jumps over the lazy dog.",
    "Rust compiles WebAssembly",
];

/// Both backends must produce bit-identical embeddings for every sentence.
///
/// `#[ignore]` + `KATAKURI_TEST_PARITY=1` because the first run downloads
/// the model twice (once per cache) over the network.
#[test]
#[ignore = "requires network on first run + KATAKURI_TEST_PARITY=1; two ~90 MB model loads"]
fn katakuri_matches_candle_bit_for_bit() {
    if std::env::var("KATAKURI_TEST_PARITY").ok().as_deref() != Some("1") {
        eprintln!("skipping: set KATAKURI_TEST_PARITY=1 to run the parity acceptance");
        return;
    }

    let candle = CandleEmbedder::from_hub().expect("load candle embedder");
    let katakuri = KatakuriEmbedder::from_hub().expect("load katakuri embedder");

    // Same default model + revision on both paths.
    assert_eq!(candle.model_id(), katakuri.model_id());

    for sentence in SENTENCES {
        let a = candle.embed(sentence).expect("candle embed");
        let b = katakuri.embed(sentence).expect("katakuri embed");

        assert_eq!(a.0.len(), b.0.len(), "dimension mismatch for {sentence:?}");
        // The enumerated element-wise equality.
        assert_eq!(a.0, b.0, "embedding mismatch for {sentence:?}");
        // Both are deterministic CPU f32 through the same candle kernels
        // with the same weights and identical token ids — anything other
        // than exact bit-equality is a port defect (tokenization, config,
        // or weights), not noise. Do not relax this to a tolerance without
        // investigating the divergence first and writing down what it was.
        for (i, (x, y)) in a.0.iter().zip(&b.0).enumerate() {
            assert!(
                x.to_bits() == y.to_bits(),
                "bit mismatch for {sentence:?} at dim {i}: candle={x} katakuri={y}"
            );
        }
    }
}
