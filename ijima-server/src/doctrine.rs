// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! Doctrine seed-pack format + ingest client.
//!
//! Doctrine entries are authored as markdown files in a Git repo (the
//! "seed pack"), reviewed via PR, and mirrored into the service. The
//! format is frontmatter + body — human-friendly for review:
//!
//! ```markdown
//! ---
//! id: doctrine-kai-vocabulary
//! project: kai
//! topic: vocabulary
//! ---
//!
//! The canonical set of terms used across the Kai ecosystem...
//! ```
//!
//! See `docs/DESIGN.md` D9 and `docs/discovery/memory-service-design.md`
//! §1-2 for the doctrine tier's role.

use std::path::{Path, PathBuf};

use ijima_core::{IjimaError, Result};

/// A parsed doctrine entry from a seed-pack markdown file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctrineEntry {
    /// Stable identifier (becomes the memory id in `ns_doctrine`).
    pub id: String,
    /// Project namespace.
    pub project: String,
    /// Topic within the project.
    pub topic: String,
    /// The body content (markdown prose).
    pub content: String,
}

/// Parses a doctrine markdown file: leading `---`-delimited frontmatter
/// (flat `key: value` lines) + body.
///
/// # Errors
///
/// Returns [`IjimaError::InvalidInput`] if the frontmatter is missing,
/// malformed, or lacks the required `id` field.
pub fn parse_doctrine_file(text: &str) -> Result<DoctrineEntry> {
    let trimmed = text.trim_start();
    let after_delim = trimmed.strip_prefix("---").ok_or_else(|| {
        IjimaError::invalid_input("doctrine file must start with --- frontmatter")
    })?;

    // Find the closing ---.
    let close = after_delim
        .find("\n---")
        .ok_or_else(|| IjimaError::invalid_input("doctrine frontmatter missing closing ---"))?;
    let frontmatter = &after_delim[..close];
    let body = after_delim[close + "\n---".len()..].trim();

    let mut id = None;
    let mut project = None;
    let mut topic = None;
    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some((k, v)) = line.split_once(':') {
            let key = k.trim();
            let val = v.trim().trim_matches('"');
            match key {
                "id" => id = Some(val.to_string()),
                "project" => project = Some(val.to_string()),
                "topic" => topic = Some(val.to_string()),
                _ => {}
            }
        }
    }

    Ok(DoctrineEntry {
        id: id.ok_or_else(|| IjimaError::invalid_input("doctrine entry missing 'id'"))?,
        project: project.unwrap_or_else(|| "doctrine".into()),
        topic: topic.unwrap_or_else(|| "general".into()),
        content: body.to_string(),
    })
}

/// Reads all `*.md` files from `dir` (non-recursive) and parses each.
/// Returns `(path, entry)` pairs so callers can report which file failed.
///
/// # Errors
///
/// Returns [`IjimaError::Store`] on I/O failure or
/// [`IjimaError::InvalidInput`] on a parse error.
pub fn read_doctrine_dir(dir: &Path) -> Result<Vec<(PathBuf, DoctrineEntry)>> {
    let mut entries = Vec::new();
    let read = std::fs::read_dir(dir).map_err(|e| IjimaError::Store {
        detail: format!("read doctrine dir {}: {e}", dir.display()),
    })?;
    for item in read {
        let path = item.map_err(|e| IjimaError::Store {
            detail: format!("readdir: {e}"),
        })?;
        let path = path.path();
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            let text = std::fs::read_to_string(&path).map_err(|e| IjimaError::Store {
                detail: format!("read {}: {e}", path.display()),
            })?;
            let entry = parse_doctrine_file(&text)?;
            entries.push((path, entry));
        }
    }
    entries.sort_by(|a, b| a.1.id.cmp(&b.1.id));
    Ok(entries)
}

/// Ingests parsed doctrine entries into a running daemon via HTTP.
/// Each entry is POSTed to `/doctrine` with the admin bearer token.
/// Returns the count successfully ingested.
///
/// Requires the `cli` feature (reqwest).
///
/// # Errors
///
/// Returns [`IjimaError::Transport`] on any HTTP failure.
#[cfg(feature = "cli")]
pub async fn ingest_to_daemon(
    url: &str,
    token: &str,
    entries: &[DoctrineEntry],
    namespace: Option<&str>,
) -> Result<usize> {
    let client = reqwest::Client::new();
    let base = format!("{}/doctrine", url.trim_end_matches('/'));
    let endpoint = match namespace {
        Some(ns) => format!("{base}?namespace={ns}"),
        None => base,
    };
    let mut count = 0;
    for entry in entries {
        // Rate limiter stays on server-side; back off on 429/503
        // (the 0.2.1 import lesson - 250ms x 2^n, six retries).
        let mut attempt = 0u32;
        loop {
            let resp = client
                .post(&endpoint)
                .bearer_auth(token)
                .json(&serde_json::json!({
                    "id": entry.id,
                    "content": entry.content,
                    "project": entry.project,
                    "topic": entry.topic,
                }))
                .send()
                .await
                .map_err(|e| IjimaError::Transport {
                    detail: format!("ingest {}: {e}", entry.id),
                })?;
            let status = resp.status().as_u16();
            if status == 429 || status == 503 {
                attempt += 1;
                if attempt > 6 {
                    return Err(IjimaError::Transport {
                        detail: format!("ingest {}: rate limited after retries", entry.id),
                    });
                }
                tokio::time::sleep(std::time::Duration::from_millis(250u64 << attempt.min(6)))
                    .await;
                continue;
            }
            resp.error_for_status().map_err(|e| IjimaError::Transport {
                detail: format!("ingest {}: {e}", entry.id),
            })?;
            break;
        }
        count += 1;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frontmatter_and_body() {
        let text =
            "---\nid: doctrine-vocab\nproject: kai\ntopic: vocabulary\n---\n\nThe canonical terms.";
        let entry = parse_doctrine_file(text).expect("parse");
        assert_eq!(entry.id, "doctrine-vocab");
        assert_eq!(entry.project, "kai");
        assert_eq!(entry.topic, "vocabulary");
        assert_eq!(entry.content, "The canonical terms.");
    }

    #[test]
    fn defaults_project_and_topic_when_absent() {
        let text = "---\nid: minimal\n---\nBody only.";
        let entry = parse_doctrine_file(text).expect("parse");
        assert_eq!(entry.id, "minimal");
        assert_eq!(entry.project, "doctrine");
        assert_eq!(entry.topic, "general");
        assert_eq!(entry.content, "Body only.");
    }

    #[test]
    fn rejects_missing_frontmatter() {
        assert!(parse_doctrine_file("no frontmatter here").is_err());
    }

    #[test]
    fn rejects_missing_closing_delimiter() {
        assert!(parse_doctrine_file("---\nid: x\nbody without close").is_err());
    }

    #[test]
    fn rejects_missing_id() {
        assert!(parse_doctrine_file("---\nproject: x\n---\nbody").is_err());
    }

    #[test]
    fn preserves_multiline_body() {
        let text = "---\nid: multi\n---\nLine one.\n\nLine two.\n- bullet";
        let entry = parse_doctrine_file(text).expect("parse");
        assert_eq!(entry.content, "Line one.\n\nLine two.\n- bullet");
    }

    #[test]
    fn strips_quoted_values() {
        let text = "---\nid: \"quoted-id\"\nproject: \"quoted project\"\ntopic: x\n---\nbody";
        let entry = parse_doctrine_file(text).expect("parse");
        assert_eq!(entry.id, "quoted-id");
        assert_eq!(entry.project, "quoted project");
    }
}
