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

// ---------- tree mode (v0.3.0 U2): markdown corpus trees ----------

/// fnmatch-style glob against a posix relpath. `*` matches any run
/// (including `/`), `?` one character, `[seq]` / `[!seq]` character
/// classes — Python-fnmatch parity minus case folding. Patterns match
/// the whole relpath.
#[cfg(feature = "cli")]
pub fn fn_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star_p, mut star_t) = (usize::MAX, 0usize);
    while ti < t.len() {
        if pi < p.len() {
            match p[pi] {
                '*' => {
                    star_p = pi;
                    star_t = ti;
                    pi += 1;
                    continue;
                }
                '?' => {
                    pi += 1;
                    ti += 1;
                    continue;
                }
                '[' => {
                    if let Some(len) = class_len(&p[pi..]) {
                        if class_matches(&p[pi..pi + len], t[ti]) {
                            pi += len;
                            ti += 1;
                            continue;
                        }
                    } else if p[pi] == t[ti] {
                        // unterminated class: literal bracket
                        pi += 1;
                        ti += 1;
                        continue;
                    }
                }
                c if c == t[ti] => {
                    pi += 1;
                    ti += 1;
                    continue;
                }
                _ => {}
            }
        }
        if star_p != usize::MAX {
            pi = star_p + 1;
            star_t += 1;
            ti = star_t;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Length of a `[...]` class starting at `p[0] == '['` (inclusive of
/// both brackets), if terminated on this pattern.
#[cfg(feature = "cli")]
fn class_len(p: &[char]) -> Option<usize> {
    let mut i = 1; // past '['
    if i < p.len() && p[i] == '!' {
        i += 1;
    }
    while i < p.len() {
        if p[i] == ']' {
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

/// Whether `p[0] == '['`-headed class matches `c`.
#[cfg(feature = "cli")]
fn class_matches(p: &[char], c: char) -> bool {
    let mut i = 1;
    let neg = p.get(1) == Some(&'!');
    if neg {
        i += 1;
    }
    let mut hit = false;
    while i < p.len() && p[i] != ']' {
        if i + 2 < p.len() && p[i + 1] == '-' && p[i + 2] != ']' {
            if p[i] <= c && c <= p[i + 2] {
                hit = true;
            }
            i += 3;
        } else {
            if p[i] == c {
                hit = true;
            }
            i += 1;
        }
    }
    hit != neg
}

/// The `id:` value if `text` opens with parseable frontmatter carrying
/// one — such files pass through verbatim; everything else is wrapped.
#[cfg(feature = "cli")]
fn frontmatter_id(text: &str) -> Option<String> {
    let trimmed = text.trim_start();
    let after = trimmed.strip_prefix("---")?;
    let close = after.find("\n---")?;
    for line in after[..close].lines() {
        let Some((key, value)) = line.trim().split_once(':') else {
            continue; // blank or key-less lines are not authoritative
        };
        if key.trim() == "id" && !value.trim().is_empty() {
            return Some(value.trim().trim_matches('"').to_string());
        }
    }
    None
}

/// Stable tree-derived id: `doct_<hash12>` of the posix relpath —
/// re-runs upsert edits in place and append additions, never duplicate.
#[cfg(feature = "cli")]
fn tree_entry_id(rel: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(rel.as_bytes());
    // sha2 0.11's `finalize()` output is not `{:x}`-formattable; hex it
    // byte-by-byte (same approach as auth.rs).
    let hex: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    format!("doct_{}", &hex[..12])
}

/// (project, topic) from the relpath segments: first segment / second
/// segment stem; top-level files fall back to the root's name.
#[cfg(feature = "cli")]
fn derive_project_topic(rel: &str, root_name: &str) -> (String, String) {
    let parts: Vec<&str> = rel.split('/').filter(|s| !s.is_empty()).collect();
    let stem = |s: &str| {
        s.rsplit_once('.')
            .map(|(head, _)| head)
            .unwrap_or(s)
            .to_string()
    };
    if parts.len() == 1 {
        (root_name.to_string(), stem(parts[0]))
    } else {
        (parts[0].to_string(), stem(parts[1]))
    }
}

/// A tree-mode ingestion plan: `(relpath, entry, verbatim)` — verbatim
/// marks files that already carried id frontmatter and passed through.
#[cfg(feature = "cli")]
pub type TreeEntry = (PathBuf, DoctrineEntry, bool);

/// Walks a corpus tree and builds doctrine entries: `.md` only, dot
/// directories skipped, include/exclude globs against posix relpaths
/// (empty includes = all `.md`; excludes always win). Files with
/// existing `id` frontmatter parse verbatim; the rest get synthesized
/// stable ids. Sorted by relpath for deterministic runs.
///
/// # Errors
///
/// Returns [`IjimaError::Store`] on I/O failure (mirroring
/// [`read_doctrine_dir`]) and propagates parse errors with the path.
#[cfg(feature = "cli")]
pub fn read_doctrine_tree(
    root: &Path,
    includes: &[String],
    excludes: &[String],
) -> Result<Vec<TreeEntry>> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
        let read = std::fs::read_dir(dir).map_err(|e| IjimaError::Store {
            detail: format!("read tree dir {}: {e}", dir.display()),
        })?;
        for item in read {
            let path = item
                .map_err(|e| IjimaError::Store {
                    detail: format!("readdir: {e}"),
                })?
                .path();
            let name_ok = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| !n.starts_with('.'))
                .unwrap_or(false);
            if !name_ok {
                continue;
            }
            if path.is_dir() {
                walk(&path, out)?;
            } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                out.push(path);
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    walk(root, &mut files)?;
    files.sort();

    let root_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("corpus")
        .to_lowercase()
        .replace(' ', "-");

    let mut entries = Vec::new();
    for path in files {
        let rel = path.strip_prefix(root).unwrap_or(&path);
        let rel_posix = rel.to_string_lossy().replace('\\', "/");
        if !includes.is_empty() && !includes.iter().any(|pat| fn_match(pat, &rel_posix)) {
            continue;
        }
        if excludes.iter().any(|pat| fn_match(pat, &rel_posix)) {
            continue;
        }
        let bytes = std::fs::read(&path).map_err(|e| IjimaError::Store {
            detail: format!("read {}: {e}", path.display()),
        })?;
        let text = String::from_utf8_lossy(&bytes).into_owned();
        if frontmatter_id(&text).is_some() {
            let entry = parse_doctrine_file(&text).map_err(|e| IjimaError::Store {
                detail: format!("parse {}: {e}", path.display()),
            })?;
            entries.push((rel.to_path_buf(), entry, true));
        } else {
            let (project, topic) = derive_project_topic(&rel_posix, &root_name);
            entries.push((
                rel.to_path_buf(),
                DoctrineEntry {
                    id: tree_entry_id(&rel_posix),
                    content: text,
                    project,
                    topic,
                },
                false,
            ));
        }
    }
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

    // ---------- tree mode (v0.3.0 U2) ----------

    #[cfg(feature = "cli")]
    mod tree {
        use super::*;

        #[test]
        fn fn_match_crosses_separators_and_anchors_whole_path() {
            assert!(fn_match(
                "RESEARCH_REPORTS/*",
                "RESEARCH_REPORTS/PULSE_2026-08-05_Anima.md"
            ));
            assert!(fn_match("RESEARCH_REPORTS/*", "RESEARCH_REPORTS/a/b.md"));
            assert!(!fn_match("A/*", "B/A/x.md")); // anchored at the front
            assert!(fn_match("*.md", "A/x.md")); // * crosses /
            assert!(fn_match("A/two.md", "A/two.md"));
            assert!(!fn_match("A/two.md", "A/two.md.bak"));
            assert!(fn_match("?.md", "a.md"));
            assert!(!fn_match("?.md", "ab.md"));
        }

        #[test]
        fn fn_match_character_classes() {
            assert!(fn_match("[a-c]x", "bx"));
            assert!(!fn_match("[a-c]x", "dx"));
            assert!(fn_match("[!0-9]*", "report-1"));
            assert!(!fn_match("[!0-9]*", "1report"));
        }

        #[test]
        fn tree_entry_id_is_stable_and_formatted() {
            let a = tree_entry_id("RESEARCH_REPORTS/PULSE_2026-08-05_Anima.md");
            let b = tree_entry_id("RESEARCH_REPORTS/PULSE_2026-08-05_Anima.md");
            assert_eq!(a, b);
            assert_eq!(a.len(), "doct_".len() + 12);
            assert!(a.starts_with("doct_"));
            assert!(a[5..].chars().all(|c| c.is_ascii_hexdigit()));
            assert_ne!(a, tree_entry_id("RESEARCH_REPORTS/OTHER.md"));
        }

        #[test]
        fn derive_project_topic_segments() {
            let (p, t) =
                derive_project_topic("RESEARCH_REPORTS/PULSE_2026-08-05_Anima.md", "ia-documents");
            assert_eq!(
                (p.as_str(), t.as_str()),
                ("RESEARCH_REPORTS", "PULSE_2026-08-05_Anima")
            );

            let (p, t) = derive_project_topic("Amari/rewrite/rewrite-ideation-session.md", "x");
            assert_eq!((p.as_str(), t.as_str()), ("Amari", "rewrite"));

            let (p, t) = derive_project_topic("README.md", "ia-documents");
            assert_eq!((p.as_str(), t.as_str()), ("ia-documents", "README"));
        }

        #[test]
        fn frontmatter_id_detection() {
            assert_eq!(
                frontmatter_id("---\nid: x\nproject: y\n---\nbody"),
                Some("x".to_string())
            );
            assert_eq!(frontmatter_id("---\ntitle: only\n---\nbody"), None);
            assert_eq!(frontmatter_id("no frontmatter"), None);
            assert_eq!(frontmatter_id("---\nid:   \n---\nb"), None); // empty id
        }

        fn scratch_dir(tag: &str) -> std::path::PathBuf {
            let d = std::env::temp_dir().join(format!(
                "ijima-doctree-{}-{}-{tag}",
                std::process::id(),
                crate_tests_counter()
            ));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            d
        }

        fn crate_tests_counter() -> u32 {
            use std::sync::atomic::{AtomicU32, Ordering};
            static N: AtomicU32 = AtomicU32::new(0);
            N.fetch_add(1, Ordering::SeqCst)
        }

        #[test]
        fn read_doctrine_tree_walks_filters_and_synthesizes() {
            let root = scratch_dir("walk");
            std::fs::create_dir_all(root.join("A")).unwrap();
            std::fs::create_dir_all(root.join(".git")).unwrap();
            std::fs::write(root.join("ROOT.md"), "top doc\n").unwrap();
            std::fs::write(root.join("A/one.md"), "nested\n").unwrap();
            std::fs::write(root.join("A/two.md"), "---\nid: manual-1\n---\nverbatim\n").unwrap();
            std::fs::write(root.join("A/three.md"), "skip me").unwrap();
            std::fs::write(root.join(".git/hidden.md"), "no").unwrap();
            std::fs::write(root.join("notes.txt"), "not md").unwrap();

            let includes = vec!["*.md".to_string()];
            let excludes = vec!["A/three.md".to_string()];
            let got = read_doctrine_tree(&root, &includes, &excludes).unwrap();
            let rels: Vec<String> = got
                .iter()
                .map(|(p, _, _)| p.to_string_lossy().into_owned())
                .collect();
            assert_eq!(rels, vec!["A/one.md", "A/two.md", "ROOT.md"]);

            let (_, one, verbatim) = &got[0];
            assert!(!verbatim);
            assert!(one.id.starts_with("doct_"));
            assert_eq!(one.project, "A");
            assert_eq!(one.topic, "one");
            assert_eq!(one.content, "nested\n");

            let (_, two, verbatim) = &got[1];
            assert!(verbatim);
            assert_eq!(two.id, "manual-1");

            let (_, top, _) = &got[2];
            assert_eq!(
                top.project,
                root.file_name().unwrap().to_str().unwrap().to_lowercase()
            ); // root-name fallback
            assert_eq!(top.topic, "ROOT");

            // Idempotence by construction: same tree, same ids.
            let again = read_doctrine_tree(&root, &includes, &excludes).unwrap();
            assert_eq!(got[0].1.id, again[0].1.id);

            let _ = std::fs::remove_dir_all(&root);
        }
    }

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
