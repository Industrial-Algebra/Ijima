// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! Proserpina-backed LLM extraction tier (ADR M5, M7, M8).
//!
//! Adds the **Fact** and **Pattern** roles that the deterministic rules tier
//! cannot produce (they need model judgment). Each role is a Proserpina
//! [`Persona`] doing a **single-shot** `respond` pass over the session turns
//! (ADR M5 — no panel cross-examination in v0).
//!
//! ## Output contract
//!
//! The persona is instructed (via its framing + the prompt) to emit one JSON
//! object per line, each describing an extraction:
//!
//! ```text
//! {"content":"Ijima uses SurrealDB for storage","project":"ijima","topic":"storage","confidence":0.8}
//! ```
//!
//! The miner parses these into [`Extraction`]s, routed per the role's default
//! ([`LlmRoute`]). Unparseable lines are skipped (the LLM is fallible; a
//! partial parse is better than failing the whole pass).
//!
//! ## Determinism
//!
//! Proserpina's `Agent` is **synchronous**, matching `mine`'s sync signature
//! (ADR M1). The real HTTP backend lives in Proserpina's `backend-http`
//! feature (constructed by the daemon); tests use a scripted agent that
//! returns canned JSON.

use ijima_core::{IjimaError, Memory, MemoryId, MemorySource, Result};
use proserpina::{Agent, AgentId, Message, MessageKind, Persona};
use serde::Deserialize;

use crate::{Extraction, MiningContext};

/// Where a role's extractions route by default (ADR M3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmRoute {
    /// Auto-archive to the palace.
    Auto,
    /// Stage in the review queue (the default for model-judged extractions).
    PendingReview,
}

/// An LLM extraction role: a Proserpera persona + a default route.
#[derive(Debug, Clone)]
pub struct LlmRole {
    /// The persona (name + framing + focus) the agent applies.
    pub persona: Persona,
    /// Default route for this role's extractions.
    pub route: LlmRoute,
}

/// The **fact-extractor** role: surfaces stated technical facts. Defaults to
/// `PendingReview` (model judgment — the reviewer confirms before promotion).
pub fn fact_extractor() -> LlmRole {
    LlmRole {
        persona: Persona::new("Fact Extractor")
            .with_framing(
                "You extract concrete technical facts stated in the session. \
                 Output one JSON object per line: \
                 {\"content\",\"project\",\"topic\",\"confidence\"}. \
                 Omit preamble. If nothing factual, output nothing.",
            )
            .with_focus("decisions, chosen tools, stated constraints, measurements"),
        route: LlmRoute::PendingReview,
    }
}

/// The **pattern-extractor** role: surfaces repeated behaviours / recurring
/// themes across turns. Defaults to `PendingReview`.
pub fn pattern_extractor() -> LlmRole {
    LlmRole {
        persona: Persona::new("Pattern Extractor")
            .with_framing(
                "You surface recurring patterns and repeated themes across the \
                 session turns. Output one JSON object per line: \
                 {\"content\",\"project\",\"topic\",\"confidence\"}. \
                 Omit preamble. If no clear pattern, output nothing.",
            )
            .with_focus("repeated workflows, recurring problems, habits, loops"),
        route: LlmRoute::PendingReview,
    }
}

/// The canonical role set for the llm tier (ADR M8): fact + pattern.
pub fn default_roles() -> Vec<LlmRole> {
    vec![fact_extractor(), pattern_extractor()]
}

/// Runs every role in `roles` over `turns` via `agent` (single-shot each,
/// ADR M5), collecting all extractions. Roles are independent.
pub fn mine_llm(
    agent: &mut dyn Agent,
    roles: &[LlmRole],
    turns: &[String],
    ctx: &MiningContext,
) -> Result<Vec<Extraction>> {
    let mut out = Vec::new();
    for role in roles {
        let mut found = extract_with_agent(agent, role, turns, ctx)?;
        out.append(&mut found);
    }
    Ok(out)
}

/// Runs a single role: builds the prompt, gets one response, parses it.
pub fn extract_with_agent(
    agent: &mut dyn Agent,
    role: &LlmRole,
    turns: &[String],
    ctx: &MiningContext,
) -> Result<Vec<Extraction>> {
    let prompt = build_prompt(role, turns);
    let msg = Message::new(
        AgentId::new("ijima-miner"),
        Some(agent.id().clone()),
        MessageKind::Prompt,
        prompt,
    );
    let response = agent.respond(&msg).map_err(mining_err)?;
    Ok(parse_response(response.text(), role, ctx))
}

/// Builds the prompt: the persona's job is in its framing; the prompt carries
/// the format reminder + the raw turns.
fn build_prompt(role: &LlmRole, turns: &[String]) -> String {
    let mut s = String::new();
    s.push_str("Extract per your framing. One JSON object per line:\n");
    s.push_str("{\"content\",\"project\",\"topic\",\"confidence\"}\n\n--- session turns ---\n");
    for (i, t) in turns.iter().enumerate() {
        s.push_str(&format!("[{i}] {t}\n"));
    }
    // Touch role to keep it conceptually tied (persona framing governs);
    // the field is read by the agent via agent.persona().
    let _ = &role.persona;
    s
}

/// A parsed line from the LLM response.
#[derive(Deserialize)]
struct LlmLine {
    content: String,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    topic: Option<String>,
    #[serde(default = "default_confidence")]
    confidence: f32,
}

fn default_confidence() -> f32 {
    0.5
}

/// Parses an agent response into [`Extraction`]s. Each non-empty line is
/// parsed as JSON; unparseable lines are silently skipped (the LLM is
/// fallible). Confidence above the role's auto-threshold overrides the role's
/// default route to `Auto` (a high-confidence fact auto-archives).
fn parse_response(text: &str, role: &LlmRole, ctx: &MiningContext) -> Vec<Extraction> {
    let auto_threshold = 0.85; // confidence at/above this auto-archives even a PendingReview role
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        // Skip blank lines and non-JSON preamble.
        if line.is_empty() || !line.starts_with('{') {
            continue;
        }
        if let Ok(parsed) = serde_json::from_str::<LlmLine>(line) {
            if parsed.content.trim().is_empty() {
                continue;
            }
            let route = if parsed.confidence >= auto_threshold {
                LlmRoute::Auto
            } else {
                role.route
            };
            let memory = llm_memory(&parsed, ctx);
            out.push(match route {
                LlmRoute::Auto => Extraction::Auto(memory),
                LlmRoute::PendingReview => Extraction::PendingReview(memory),
            });
        }
    }
    out
}

fn llm_memory(line: &LlmLine, ctx: &MiningContext) -> Memory {
    Memory {
        id: MemoryId(format!("mined_llm_{}", short_hash(&line.content))),
        content: line.content.clone(),
        project: line.project.clone().unwrap_or_else(|| ctx.project.clone()),
        topic: line.topic.clone().unwrap_or_else(|| "mined".to_string()),
        source: MemorySource::Mined,
        harness: ctx.harness,
        session_id: Some(ctx.session_id.clone()),
        importance: line.confidence.clamp(0.0, 1.0),
        created_at: ctx.now.clone(),
    }
}

fn short_hash(s: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:x}")
}

fn mining_err(e: proserpina::ProserpinaError) -> IjimaError {
    IjimaError::Mining {
        detail: format!("llm extraction: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ijima_core::harness::Harness;

    fn ctx() -> MiningContext {
        MiningContext {
            session_id: "sess_1".into(),
            project: "ijima".into(),
            harness: Harness::Pi,
            now: "0".into(),
        }
    }

    /// A scripted agent that ignores its input and returns a fixed response.
    /// Lets us test parsing/routing deterministically without an LLM.
    struct ScriptedAgent {
        id: AgentId,
        persona: Persona,
        response: String,
    }

    impl Agent for ScriptedAgent {
        fn id(&self) -> &AgentId {
            &self.id
        }
        fn persona(&self) -> &Persona {
            &self.persona
        }
        fn respond(
            &mut self,
            _msg: &Message,
        ) -> std::result::Result<Message, proserpina::ProserpinaError> {
            Ok(Message::new(
                self.id.clone(),
                Some(AgentId::new("ijima-miner")),
                MessageKind::Critique,
                self.response.clone(),
            ))
        }
    }

    fn scripted(response: &str) -> ScriptedAgent {
        ScriptedAgent {
            id: AgentId::new("test-agent"),
            persona: Persona::new("Fact Extractor"),
            response: response.to_string(),
        }
    }

    #[test]
    fn parses_json_lines_into_pending_review() {
        let role = fact_extractor(); // default route PendingReview
        let resp = "{\"content\":\"Ijima uses SurrealDB\",\"project\":\"ijima\",\"topic\":\"storage\",\"confidence\":0.7}\n\
                    not json, skipped\n\
                    {\"content\":\"another fact\",\"confidence\":0.6}\n";
        let got = parse_response(resp, &role, &ctx());
        assert_eq!(got.len(), 2);
        assert!(
            got.iter()
                .all(|e| matches!(e, Extraction::PendingReview(_)))
        );
        match &got[0] {
            Extraction::PendingReview(m) => {
                assert_eq!(m.content, "Ijima uses SurrealDB");
                assert_eq!(m.topic, "storage");
            }
            _ => unreachable!(),
        }
        // second line used the ctx project default
        match &got[1] {
            Extraction::PendingReview(m) => assert_eq!(m.project, "ijima"),
            _ => unreachable!(),
        }
    }

    #[test]
    fn high_confidence_auto_archives() {
        let role = fact_extractor(); // default PendingReview
        let resp = "{\"content\":\"very certain fact\",\"confidence\":0.9}";
        let got = parse_response(resp, &role, &ctx());
        assert_eq!(got.len(), 1);
        assert!(matches!(got[0], Extraction::Auto(_)));
    }

    #[test]
    fn empty_response_yields_nothing() {
        let role = pattern_extractor();
        assert!(parse_response("", &role, &ctx()).is_empty());
        assert!(parse_response("just prose, no json", &role, &ctx()).is_empty());
    }

    #[test]
    fn extract_with_agent_end_to_end() {
        let mut agent = scripted(
            "{\"content\":\"pattern: always commits after green CI\",\"topic\":\"workflow\",\"confidence\":0.5}",
        );
        let role = pattern_extractor();
        let turns = vec!["turn one".to_string(), "turn two".to_string()];
        let got = extract_with_agent(&mut agent, &role, &turns, &ctx()).expect("extract");
        assert_eq!(got.len(), 1);
        match &got[0] {
            Extraction::PendingReview(m) => {
                assert!(m.content.contains("always commits after green CI"));
                assert_eq!(m.topic, "workflow");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn mine_llm_runs_all_roles() {
        let mut agent = scripted(
            "{\"content\":\"a fact\",\"confidence\":0.6}\n{\"content\":\"a pattern\",\"confidence\":0.6}",
        );
        let roles = default_roles(); // fact + pattern
        let turns = vec!["t".to_string()];
        let got = mine_llm(&mut agent, &roles, &turns, &ctx()).expect("mine_llm");
        // both roles return both lines → 4 extractions
        assert_eq!(got.len(), 4);
    }
}
