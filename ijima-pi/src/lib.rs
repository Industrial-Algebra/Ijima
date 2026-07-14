// Copyright 2026 Industrial Algebra. Licensed under Apache-2.0.

//! Ijima-pi WebAssembly core — pure request/response shape mapping between
//! Ijima's REST API and pi's tool surface.
//!
//! Architecture: **path (b)** — no HTTP, no tokio, no reqwest. This crate
//! owns the type-safe serde translation layer. The TS shim (`integrations/pi/`)
//! holds the HTTP fetch + pi registration. See
//! [`docs/plans/2026-07-12-pi-integration.md`] §2.
#![deny(unsafe_code)]

use ijima_core::store::SearchHit;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

// ---------------------------------------------------------------------------
// Internal shapes (own the serde contract for the integration, not general pub)
// ---------------------------------------------------------------------------

/// Ijima `POST /memories/search` response wrapper.
/// The server API returns `{ "memories": [...] }`; this deserializes it.
#[derive(Deserialize)]
struct SearchResponse {
    memories: Vec<SearchHit>,
}

/// Pi-friendly memory hit — the shape the pi `memory_search` tool returns.
/// Dropped fields (id, source, harness, session_id, origin, authority) are
/// internal provenance; the LLM only needs a displayable subset.
#[derive(Serialize)]
struct PiMemoryHit {
    text: String,
    project: String,
    topic: String,
    timestamp: String,
    importance: f32,
    similarity: f32,
}

// ---------------------------------------------------------------------------
// wasm-bindgen exports (called from the TS shim)
// ---------------------------------------------------------------------------

/// Build the JSON request body for `POST /memories/search`.
///
/// `scope` defaults to `"visible"` — the pi integration's path through Ijima's
/// multi-namespace merge (§3.5). The TS shim never overrides this to `personal`;
/// `visible` is what restores pi-mempalace global-search parity.
#[wasm_bindgen]
pub fn build_search_request(text: String, limit: Option<usize>, scope: Option<String>) -> String {
    let body = serde_json::json!({
        "text": text,
        "limit": limit.unwrap_or(10),
        "scope": scope.unwrap_or_else(|| "visible".to_string()),
    });
    body.to_string()
}

/// Parse Ijima's `POST /memories/search` JSON response into pi-friendly JSON.
///
/// Expects an Ijima `SearchResponse` (`{ "memories": [{ "memory": {...}, "similarity": ... }] }`).
/// Returns `[{ "text", "project", "topic", "timestamp", "importance", "similarity" }]`.
/// On parse failure returns `{ "error": "..." }`.
#[wasm_bindgen]
pub fn parse_search_response(json_str: String) -> String {
    let response: SearchResponse = match serde_json::from_str(&json_str) {
        Ok(r) => r,
        Err(e) => {
            return serde_json::json!({ "error": e.to_string() }).to_string();
        }
    };

    let hits: Vec<PiMemoryHit> = response
        .memories
        .into_iter()
        .map(|h| PiMemoryHit {
            text: h.memory.content,
            project: h.memory.project,
            topic: h.memory.topic,
            timestamp: h.memory.created_at,
            importance: h.memory.importance,
            similarity: h.similarity,
        })
        .collect();

    serde_json::to_string(&hits).unwrap_or_else(|e| {
        serde_json::json!({ "error": e.to_string() }).to_string()
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_search_request_defaults_visible() {
        let json = build_search_request("hello".into(), None, None);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["text"], "hello");
        assert_eq!(v["limit"], 10);
        assert_eq!(v["scope"], "visible");
    }

    #[test]
    fn build_search_request_explicit_limit() {
        let json = build_search_request("hello".into(), Some(5), Some("personal".into()));
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["limit"], 5);
        assert_eq!(v["scope"], "personal");
    }

    #[test]
    fn parse_search_response_happy_path() {
        let input = r#"{"memories":[{"memory":{"id":"mem_01","content":"hello world","project":"test","topic":"demo","source":"Explicit","harness":"Pi","session_id":null,"origin":"local","authority":"local","importance":0.8,"created_at":"1712345678"},"similarity":0.92}]}"#;
        let output = parse_search_response(input.into());
        let hits: Vec<serde_json::Value> = serde_json::from_str(&output).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0]["text"], "hello world");
        assert_eq!(hits[0]["project"], "test");
        assert_eq!(hits[0]["topic"], "demo");
        assert_eq!(hits[0]["timestamp"], "1712345678");
        assert_eq!(hits[0]["importance"], 0.8);
        assert_eq!(hits[0]["similarity"], 0.92);
    }

    #[test]
    fn parse_search_response_bad_json() {
        let output = parse_search_response("not json".into());
        let v: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(v.get("error").is_some());
    }

    #[test]
    fn parse_search_response_empty_memories() {
        let input = r#"{"memories":[]}"#;
        let output = parse_search_response(input.into());
        let hits: Vec<serde_json::Value> = serde_json::from_str(&output).unwrap();
        assert!(hits.is_empty());
    }
}
