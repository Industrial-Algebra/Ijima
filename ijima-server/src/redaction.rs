// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! Redaction filter for the personal → shared promotion boundary.
//!
//! Per `docs/discovery/memory-service-design.md` §2 and D9, this is the
//! **one place** content filtering happens. Personal memories are stored
//! verbatim ("store everything"); when an author promotes a memory to a
//! shared namespace, the redactor scrubs secrets and PII first. Never at
//! auto-capture.
//!
//! ## Rules (v0, regex-based)
//!
//! | Category | Pattern |
//! |---|---|
//! | `api_key` | OpenAI `sk-...`, AWS `AKIA...`, generic 40+ hex/token |
//! | `bearer_token` | `Bearer <token>` |
//! | `private_key` | PEM `-----BEGIN ... PRIVATE KEY-----` blocks |
//! | `email` | RFC-ish email addresses |
//! | `ipv4` | Dotted-quad addresses |
//!
//! Replaced with `[REDACTED:<category>]`. A richer detector (secret-
//! scanning service, NER for PII) can swap in behind the same
//! [`Redactor`] interface later.

use serde::Serialize;

/// One category of redaction that fired, with a count.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Redaction {
    /// The rule category that matched (e.g. `"api_key"`, `"email"`).
    pub category: &'static str,
    /// How many matches were replaced.
    pub count: usize,
}

/// The outcome of redacting a text.
#[derive(Debug, Clone)]
pub struct RedactionResult {
    /// The scrubbed text.
    pub text: String,
    /// Which categories fired and how many times.
    pub redactions: Vec<Redaction>,
}

/// A rule-based content scrubber.
pub struct Redactor {
    rules: Vec<Rule>,
}

struct Rule {
    category: &'static str,
    pattern: regex::Regex,
    replacement: String,
}

impl Default for Redactor {
    fn default() -> Self {
        Self::new()
    }
}

impl Redactor {
    /// Constructs the standard ruleset.
    #[allow(clippy::needless_pass_by_value)]
    pub fn new() -> Self {
        let rules = vec![
            Rule {
                category: "private_key",
                pattern: regex::Regex::new(
                    r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----",
                )
                .expect("private_key regex"),
                replacement: "[REDACTED:private_key]".into(),
            },
            Rule {
                category: "bearer_token",
                pattern: regex::Regex::new(r"(?i)bearer [a-zA-Z0-9._\-]{20,}")
                    .expect("bearer regex"),
                replacement: "[REDACTED:bearer_token]".into(),
            },
            Rule {
                category: "api_key",
                pattern: regex::Regex::new(r"(sk-[a-zA-Z0-9]{20,}|AKIA[0-9A-Z]{16}|[a-f0-9]{40})")
                    .expect("api_key regex"),
                replacement: "[REDACTED:api_key]".into(),
            },
            Rule {
                category: "email",
                pattern: regex::Regex::new(r"[a-zA-Z0-9._%+\-]+@[a-zA-Z0-9.\-]+\.[a-zA-Z]{2,}")
                    .expect("email regex"),
                replacement: "[REDACTED:email]".into(),
            },
            Rule {
                category: "ipv4",
                pattern: regex::Regex::new(r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b")
                    .expect("ipv4 regex"),
                replacement: "[REDACTED:ipv4]".into(),
            },
        ];
        Self { rules }
    }

    /// Scrubs `text`, returning the redacted result + a summary of what
    /// was removed. Clean text passes through unchanged with an empty
    /// redactions list.
    pub fn redact(&self, text: &str) -> RedactionResult {
        let mut scrubbed = text.to_string();
        let mut redactions = Vec::new();
        for rule in &self.rules {
            let count = rule.pattern.find_iter(&scrubbed.clone()).count();
            if count > 0 {
                scrubbed = rule
                    .pattern
                    .replace_all(&scrubbed, &rule.replacement)
                    .into_owned();
                redactions.push(Redaction {
                    category: rule.category,
                    count,
                });
            }
        }
        RedactionResult {
            text: scrubbed,
            redactions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_text_passes_through() {
        let r = Redactor::new();
        let result = r.redact("The cat sat on the mat");
        assert_eq!(result.text, "The cat sat on the mat");
        assert!(result.redactions.is_empty());
    }

    #[test]
    fn email_is_redacted() {
        let r = Redactor::new();
        let result = r.redact("contact elliott@example.com for details");
        assert!(result.text.contains("[REDACTED:email]"));
        assert!(!result.text.contains("elliott@example.com"));
        assert_eq!(
            result.redactions,
            vec![Redaction {
                category: "email",
                count: 1
            }]
        );
    }

    #[test]
    fn openai_api_key_is_redacted() {
        let r = Redactor::new();
        let result = r.redact("key: sk-abcdefghijklmnopqrstuvwxyz1234567890");
        assert!(result.text.contains("[REDACTED:api_key]"));
        assert!(!result.text.contains("sk-abcdef"));
    }

    #[test]
    fn aws_key_is_redacted() {
        let r = Redactor::new();
        let result = r.redact("creds: AKIAIOSFODNN7EXAMPLE");
        assert!(result.text.contains("[REDACTED:api_key]"));
    }

    #[test]
    fn hex_sha1_is_redacted() {
        let r = Redactor::new();
        let result = r.redact("commit: a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2");
        assert!(result.text.contains("[REDACTED:api_key]"));
    }

    #[test]
    fn bearer_token_is_redacted() {
        let r = Redactor::new();
        let result = r.redact("Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.payload.sig");
        assert!(result.text.contains("[REDACTED:bearer_token]"));
        assert!(!result.text.contains("eyJhbG"));
    }

    #[test]
    fn private_key_block_is_redacted() {
        let r = Redactor::new();
        let text =
            "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA...\n-----END RSA PRIVATE KEY-----";
        let result = r.redact(text);
        assert!(result.text.contains("[REDACTED:private_key]"));
        assert!(!result.text.contains("MIIEpA"));
    }

    #[test]
    fn ipv4_is_redacted() {
        let r = Redactor::new();
        let result = r.redact("server at 10.0.0.5 is down");
        assert!(result.text.contains("[REDACTED:ipv4]"));
        assert!(!result.text.contains("10.0.0.5"));
    }

    #[test]
    fn multiple_categories_in_one_text() {
        let r = Redactor::new();
        let text = "email alice@test.com and use key sk-abcdefghijklmnopqrstuvwxyz1234567890";
        let result = r.redact(text);
        let cats: Vec<&str> = result.redactions.iter().map(|x| x.category).collect();
        assert!(cats.contains(&"email"));
        assert!(cats.contains(&"api_key"));
        assert!(!result.text.contains("alice@test.com"));
        assert!(!result.text.contains("sk-abc"));
    }

    #[test]
    fn version_numbers_are_not_redacted_as_api_keys() {
        // 0.1.0 should not be mistaken for a 40-hex api key.
        let r = Redactor::new();
        let result = r.redact("version 0.1.0 released");
        assert_eq!(result.text, "version 0.1.0 released");
        // ipv4 may catch it, but that's acceptable for a version that
        // looks like an IP — the point is no false api_key hit.
        assert!(!result.redactions.iter().any(|x| x.category == "api_key"));
    }
}
