// Copyright 2026 Industrial Algebra. Licensed under Apache-2.0.

//! Ijima-pi WebAssembly core — pure request/response shape mapping between
//! Ijima's REST API and pi's tool surface.
//!
//! Architecture: **path (b)** — no HTTP, no tokio, no reqwest. This crate
//! owns the type-safe serde translation layer. The TS shim (`integrations/pi/`)
//! holds the HTTP fetch + pi registration. See
//! [`docs/plans/2026-07-12-pi-integration.md`] §2.
#![deny(unsafe_code)]

use ijima_core::knowledge::Triple;
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

/// Pi-friendly knowledge triple — a displayable subset of Ijima's `Triple`.
#[derive(Serialize)]
struct PiTriple {
    id: String,
    subject: String,
    predicate: String,
    object: String,
    valid_from: Option<String>,
    valid_to: Option<String>,
    confidence: f32,
}

/// Pi-friendly entity query result.
#[derive(Serialize)]
struct PiEntityRecord {
    entity_name: Option<String>,
    entity_type: Option<String>,
    outgoing: Vec<PiTriple>,
    incoming: Vec<PiTriple>,
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

    serde_json::to_string(&hits)
        .unwrap_or_else(|e| serde_json::json!({ "error": e.to_string() }).to_string())
}

// ---------------------------------------------------------------------------
// memory_save — POST /memories (memory:write)
// ---------------------------------------------------------------------------

/// Build the JSON body for `POST /memories` (save a new memory).
/// The daemon fills `created_at` if empty; `origin`/`authority` default
/// server-side. The caller MUST supply a unique id (e.g. `mem_<ulid>`).
#[wasm_bindgen]
pub fn build_save_request(
    id: String,
    content: String,
    project: String,
    topic: String,
    importance: Option<f32>,
) -> String {
    let body = serde_json::json!({
        "id": id,
        "content": content,
        "project": project,
        "topic": topic,
        "source": "Explicit",
        "harness": "Pi",
        "session_id": null,
        "importance": importance.unwrap_or(0.8),
        "created_at": "",
    });
    body.to_string()
}

/// Parse the `POST /memories` response — just extracts the assigned id.
#[wasm_bindgen]
pub fn parse_save_response(json_str: String) -> String {
    let v: serde_json::Value = match serde_json::from_str(&json_str) {
        Ok(v) => v,
        Err(e) => return serde_json::json!({ "error": e.to_string() }).to_string(),
    };
    serde_json::json!({ "id": v["id"].as_str().unwrap_or("") }).to_string()
}

// ---------------------------------------------------------------------------
// memory_check_duplicate — POST /memories/check (memory:read)
// ---------------------------------------------------------------------------

/// Build the JSON body for `POST /memories/check`.
#[wasm_bindgen]
pub fn build_check_duplicate_request(content: String) -> String {
    serde_json::json!({ "content": content }).to_string()
}

/// Parse the duplicate-check response.
#[wasm_bindgen]
pub fn parse_check_duplicate_response(json_str: String) -> String {
    let v: serde_json::Value = match serde_json::from_str(&json_str) {
        Ok(v) => v,
        Err(e) => return serde_json::json!({ "error": e.to_string() }).to_string(),
    };
    let duplicate = v["duplicate"].as_str().map(|s| s.to_string());
    serde_json::json!({ "duplicate": duplicate }).to_string()
}

// ---------------------------------------------------------------------------
// knowledge_add — POST /kg/triples (knowledge:write)
// ---------------------------------------------------------------------------

/// Build the JSON body for `POST /kg/triples`.
#[wasm_bindgen]
pub fn build_knowledge_add_request(
    subject: String,
    predicate: String,
    object: String,
    valid_from: Option<String>,
    confidence: Option<f32>,
) -> String {
    let body = serde_json::json!({
        "subject": subject,
        "predicate": predicate,
        "object": object,
        "valid_from": valid_from,
        "confidence": confidence.unwrap_or(1.0),
    });
    body.to_string()
}

/// Parse the `POST /kg/triples` response into a pi-friendly triple.
#[wasm_bindgen]
pub fn parse_knowledge_add_response(json_str: String) -> String {
    let triple: Triple = match serde_json::from_str(&json_str) {
        Ok(t) => t,
        Err(e) => return serde_json::json!({ "error": e.to_string() }).to_string(),
    };
    let pi = PiTriple {
        id: triple.id,
        subject: triple.subject.0,
        predicate: triple.predicate,
        object: triple.object.0,
        valid_from: triple.valid_from,
        valid_to: triple.valid_to,
        confidence: triple.confidence,
    };
    serde_json::to_string(&pi)
        .unwrap_or_else(|e| serde_json::json!({ "error": e.to_string() }).to_string())
}

// ---------------------------------------------------------------------------
// knowledge_query — GET /kg/entities/{id} (knowledge:read)
// ---------------------------------------------------------------------------

/// Parse the entity query response into a pi-friendly result.
/// Uses raw JSON access for resilience — avoids coupling to Entity's
/// exact field set (the daemon includes fields like `namespace` that
/// pi doesn't need). This also sidesteps the strict-deserialization
/// requirement that all Entity fields be present.
#[wasm_bindgen]
pub fn parse_knowledge_query_response(json_str: String) -> String {
    let v: serde_json::Value = match serde_json::from_str(&json_str) {
        Ok(v) => v,
        Err(e) => return serde_json::json!({ "error": e.to_string() }).to_string(),
    };
    let map_triple = |v: &serde_json::Value| PiTriple {
        id: v["id"].as_str().unwrap_or("").to_string(),
        subject: v["subject"].as_str().unwrap_or("").to_string(),
        predicate: v["predicate"].as_str().unwrap_or("").to_string(),
        object: v["object"].as_str().unwrap_or("").to_string(),
        valid_from: v["valid_from"].as_str().map(|s| s.to_string()),
        valid_to: v["valid_to"].as_str().map(|s| s.to_string()),
        confidence: v["confidence"].as_f64().unwrap_or(1.0) as f32,
    };
    let outgoing = v["outgoing"]
        .as_array()
        .map_or(vec![], |a| a.iter().map(map_triple).collect());
    let incoming = v["incoming"]
        .as_array()
        .map_or(vec![], |a| a.iter().map(map_triple).collect());
    let pi = PiEntityRecord {
        entity_name: v["entity"]["name"].as_str().map(|s| s.to_string()),
        entity_type: v["entity"]["entity_type"].as_str().map(|s| s.to_string()),
        outgoing,
        incoming,
    };
    serde_json::to_string(&pi)
        .unwrap_or_else(|e| serde_json::json!({ "error": e.to_string() }).to_string())
}

// ---------------------------------------------------------------------------
// knowledge_timeline — GET /kg/timeline (knowledge:read)
// ---------------------------------------------------------------------------

/// Parse the timeline response into a pi-friendly triple list.
#[wasm_bindgen]
pub fn parse_knowledge_timeline_response(json_str: String) -> String {
    let triples: Vec<Triple> = match serde_json::from_str(&json_str) {
        Ok(t) => t,
        Err(e) => return serde_json::json!({ "error": e.to_string() }).to_string(),
    };
    let pi: Vec<PiTriple> = triples
        .into_iter()
        .map(|t| PiTriple {
            id: t.id,
            subject: t.subject.0,
            predicate: t.predicate,
            object: t.object.0,
            valid_from: t.valid_from,
            valid_to: t.valid_to,
            confidence: t.confidence,
        })
        .collect();
    serde_json::to_string(&pi)
        .unwrap_or_else(|e| serde_json::json!({ "error": e.to_string() }).to_string())
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

    // ----- memory_save -----

    #[test]
    fn build_save_request_defaults() {
        let json = build_save_request(
            "mem_x".into(),
            "hello".into(),
            "proj".into(),
            "top".into(),
            None,
        );
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["id"], "mem_x");
        assert_eq!(v["content"], "hello");
        assert_eq!(v["project"], "proj");
        assert_eq!(v["topic"], "top");
        assert_eq!(v["source"], "Explicit");
        assert_eq!(v["harness"], "Pi");
        assert!((v["importance"].as_f64().unwrap() - 0.8).abs() < 0.001);
        assert_eq!(v["created_at"], "");
    }

    #[test]
    fn build_save_request_explicit_importance() {
        let json = build_save_request("m".into(), "x".into(), "p".into(), "t".into(), Some(0.5));
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!((v["importance"].as_f64().unwrap() - 0.5).abs() < 0.001);
    }

    #[test]
    fn parse_save_response_happy() {
        let output = parse_save_response(r#"{"id":"mem_xyz"}"#.into());
        let v: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(v["id"], "mem_xyz");
    }

    // ----- memory_check_duplicate -----

    #[test]
    fn build_check_duplicate_request_works() {
        let json = build_check_duplicate_request("test content".into());
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["content"], "test content");
    }

    #[test]
    fn parse_check_duplicate_present() {
        let output = parse_check_duplicate_response(r#"{"duplicate":"mem_01"}"#.into());
        let v: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(v["duplicate"], "mem_01");
    }

    #[test]
    fn parse_check_duplicate_absent() {
        let output = parse_check_duplicate_response(r#"{"duplicate":null}"#.into());
        let v: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(v["duplicate"].is_null());
    }

    // ----- knowledge_add -----

    #[test]
    fn build_knowledge_add_request_works() {
        let json = build_knowledge_add_request(
            "A".into(),
            "uses".into(),
            "B".into(),
            Some("2025-01-01".into()),
            Some(0.9),
        );
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["subject"], "A");
        assert_eq!(v["predicate"], "uses");
        assert_eq!(v["object"], "B");
        assert_eq!(v["valid_from"], "2025-01-01");
        assert!((v["confidence"].as_f64().unwrap() - 0.9).abs() < 0.001);
    }

    #[test]
    fn build_knowledge_add_request_defaults() {
        let json = build_knowledge_add_request("A".into(), "uses".into(), "B".into(), None, None);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["confidence"], 1.0);
        assert!(v["valid_from"].is_null());
    }

    #[test]
    fn parse_knowledge_add_response_happy() {
        let input = r#"{"id":"triple_1","subject":"A","predicate":"uses","object":"B","valid_from":null,"valid_to":null,"confidence":1.0,"namespace":"ns_x","source_memory_id":null}"#;
        let output = parse_knowledge_add_response(input.into());
        let v: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(v["id"], "triple_1");
        assert_eq!(v["subject"], "A");
        assert_eq!(v["predicate"], "uses");
        assert_eq!(v["object"], "B");
    }

    // ----- knowledge_query -----

    #[test]
    fn parse_knowledge_query_response_happy() {
        let input = r#"{"entity":{"id":"A","name":"Alpha","entity_type":"project"},"outgoing":[],"incoming":[]}"#;
        let output = parse_knowledge_query_response(input.into());
        let v: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(v["entity_name"], "Alpha");
        assert_eq!(v["entity_type"], "project");
        assert_eq!(v["outgoing"].as_array().unwrap().len(), 0);
        assert_eq!(v["incoming"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn parse_knowledge_query_response_no_entity() {
        let input = r#"{"entity":null,"outgoing":[],"incoming":[]}"#;
        let output = parse_knowledge_query_response(input.into());
        let v: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(v["entity_name"].is_null());
        assert!(v["entity_type"].is_null());
    }

    // ----- knowledge_timeline -----

    #[test]
    fn parse_knowledge_timeline_response_happy() {
        let input = r#"[{"id":"t1","subject":"X","predicate":"depends_on","object":"Y","valid_from":null,"valid_to":null,"confidence":0.8,"namespace":"ns","source_memory_id":null}]"#;
        let output = parse_knowledge_timeline_response(input.into());
        let hits: Vec<serde_json::Value> = serde_json::from_str(&output).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0]["id"], "t1");
        assert_eq!(hits[0]["subject"], "X");
        assert_eq!(hits[0]["predicate"], "depends_on");
        assert_eq!(hits[0]["object"], "Y");
    }

    #[test]
    fn parse_knowledge_timeline_response_empty() {
        let output = parse_knowledge_timeline_response("[]".into());
        let hits: Vec<serde_json::Value> = serde_json::from_str(&output).unwrap();
        assert!(hits.is_empty());
    }
}
