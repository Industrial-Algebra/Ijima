// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! The Context Mapper — a global registry mapping repo names to filesystem
//! paths, remote URLs, and roles (the "canonical Anima ecosystem member
//! list").
//!
//! This solves the harness directory-context problem: when a harness (Pi,
//! Wallace, OpenCode) starts in a working directory, it reverse-resolves the
//! path to a canonical repo identity via Ijima, then loads that repo's mined
//! context. It also completes the path ↔ `project` link that memories and
//! sessions already key on.
//!
//! **Identity model:** the *stable* identity is the remote URL (repos get
//! cloned/moved — that's the problem being solved); `path` is a mutable
//! *current location*. `name` is the human PK.

/// A registered repository: name ↔ location ↔ identity ↔ role.
///
/// The registry is **global** (not namespace-scoped) — it is the canonical
/// ecosystem roster, admin-managed, readable by any authenticated principal.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RepoDirectory {
    /// Canonical name (primary key), e.g. `Ijima`, `Kagome`, `Tsume`.
    pub name: String,
    /// Current filesystem location (mutable). Normalized without a trailing
    /// slash on write.
    pub path: String,
    /// Stable identity: the `git remote` URL. More durable than `path`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub remote_url: Option<String>,
    /// Ecological role, e.g. `tui-substrate`, `gateway-adapter`, `orchestrator`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub role: Option<String>,
    /// Whether this repo is part of the Anima ecosystem (the roster filter).
    #[cfg_attr(feature = "serde", serde(default = "default_anima_member"))]
    pub is_anima_member: bool,
}

#[cfg(feature = "serde")]
fn default_anima_member() -> bool {
    true
}

impl RepoDirectory {
    /// Constructs a repo, normalizing the path (no trailing slash).
    pub fn new(name: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            path: normalize_path(&path.into()),
            remote_url: None,
            role: None,
            is_anima_member: true,
        }
    }

    /// Sets the remote URL (stable identity).
    #[must_use]
    pub fn with_remote(mut self, url: impl Into<String>) -> Self {
        self.remote_url = Some(url.into());
        self
    }

    /// Sets the ecological role.
    #[must_use]
    pub fn with_role(mut self, role: impl Into<String>) -> Self {
        self.role = Some(role.into());
        self
    }
}

/// Normalizes a filesystem path: no trailing slash (except root `/`).
pub fn normalize_path(path: &str) -> String {
    let p = path.trim();
    if p.len() > 1 {
        p.trim_end_matches('/').to_string()
    } else {
        p.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_normalizes_trailing_slash() {
        let r = RepoDirectory::new("Ijima", "/home/x/Ijima/");
        assert_eq!(r.path, "/home/x/Ijima");
        assert!(r.is_anima_member);
    }

    #[test]
    fn builders_set_optional_fields() {
        let r = RepoDirectory::new("Kagome", "/k")
            .with_remote("git@github.com:Anima/Kagome.git")
            .with_role("hardware");
        assert_eq!(
            r.remote_url.as_deref(),
            Some("git@github.com:Anima/Kagome.git")
        );
        assert_eq!(r.role.as_deref(), Some("hardware"));
    }

    #[test]
    fn root_path_preserved() {
        assert_eq!(normalize_path("/"), "/");
    }
}
