// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! Mining trigger orchestration (ADR M1, M3, M7).
//!
//! The async glue between the (pure, sync) extraction engine and the (async)
//! store. A trigger: fetches a session's turns, runs `ijima_miner::mine`, and
//! ingests the results — `Auto` extractions go straight to the palace
//! (`store_memory`, content-hash dedup applies), `PendingReview` extractions
//! stage in the review queue (`enqueue_extraction`).

use ijima_core::{NamespaceId, Result, Store};
use ijima_miner::{Extraction, MiningContext};

/// The outcome of a mining pass: how many extractions were auto-archived vs
/// queued for review.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MiningReport {
    /// Extractions auto-archived to the palace.
    pub archived: usize,
    /// Extractions staged in the review queue.
    pub queued: usize,
}

/// Mines `turns` (already fetched from the session) and ingests the results
/// into `store` under `ns`.
///
/// `ctx` supplies the provenance stamped onto every extraction. Returns a
/// [`MiningReport`] counting archived vs queued extractions.
///
/// # Errors
///
/// Propagates store errors from `store_memory` / `enqueue_extraction`, or a
/// mining error from the extraction engine.
pub async fn ingest_extractions(
    store: &dyn Store,
    ns: &NamespaceId,
    extractions: Vec<Extraction>,
) -> Result<MiningReport> {
    let mut report = MiningReport::default();
    for extraction in extractions {
        match extraction {
            Extraction::Auto(memory) => {
                store.store_memory(ns, memory).await?;
                report.archived += 1;
            }
            Extraction::PendingReview(memory) => {
                // Confidence is not carried on Extraction in v0 (ADR M6);
                // PendingReview defaults to a mid confidence for the reviewer.
                store.enqueue_extraction(ns, memory, 0.5).await?;
                report.queued += 1;
            }
            Extraction::Nothing => {}
        }
    }
    Ok(report)
}

/// Builds a [`MiningContext`] from the request-time facts. The caller fills
/// `project` (best-effort) and `session_id`.
pub fn mining_context(
    session_id: &str,
    project: &str,
    harness: ijima_core::harness::Harness,
) -> MiningContext {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default();
    MiningContext {
        session_id: session_id.to_string(),
        project: project.to_string(),
        harness,
        now,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ijima_core::{Memory, MemoryId, MemorySource};
    use ijima_miner::mine;

    fn sample_memory(id: &str, content: &str) -> Memory {
        Memory {
            id: MemoryId(id.into()),
            content: content.into(),
            project: "ijima".into(),
            topic: "decisions".into(),
            source: MemorySource::Mined,
            harness: ijima_core::harness::Harness::Pi,
            session_id: Some("sess_1".into()),
            origin: ijima_core::InstanceId::local(),
            authority: ijima_core::AuthorityScope::local(),
            importance: 0.7,
            created_at: "0".into(),
        }
    }

    #[tokio::test]
    async fn ingest_archives_auto_and_queues_pending() {
        // Use the real SurrealStore (in-memory) to exercise the full path.
        let store = crate::SurrealStore::open_embedded().await.expect("open");
        let ns = NamespaceId::new("ns_trigger");
        let extractions = vec![
            Extraction::Auto(sample_memory("m1", "auto decision")),
            Extraction::PendingReview(sample_memory("m2", "pending fact")),
            Extraction::Nothing,
        ];
        let report = ingest_extractions(&store, &ns, extractions)
            .await
            .expect("ingest");
        assert_eq!(
            report,
            MiningReport {
                archived: 1,
                queued: 1
            }
        );

        // Auto landed in the palace.
        let got = store
            .recall_memory(&ns, &MemoryId("m1".into()))
            .await
            .expect("recall");
        assert!(got.is_some());
        // Pending is in the queue.
        let pending = store.list_pending(&ns, 10).await.expect("list");
        assert_eq!(pending.len(), 1);
    }

    #[tokio::test]
    async fn full_pipeline_mine_then_ingest() {
        // End-to-end: real turns → mine (rules) → ingest.
        let store = crate::SurrealStore::open_embedded().await.expect("open");
        let ns = NamespaceId::new("ns_e2e");
        let ctx = mining_context("sess_1", "ijima", ijima_core::harness::Harness::Pi);
        let turns = vec![
            "We decided to use SurrealDB for storage.".to_string(),
            "See https://example.com/loader for the candle loader.".to_string(),
        ];
        let extractions = mine(&turns, &ctx).expect("mine");
        // Rules tier: 1 decision + 1 reference = 2, both Auto.
        assert_eq!(extractions.len(), 2);
        let report = ingest_extractions(&store, &ns, extractions)
            .await
            .expect("ingest");
        assert_eq!(report.archived, 2);
        assert_eq!(report.queued, 0);
    }
}
