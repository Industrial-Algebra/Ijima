// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! Schubert geometric rate limiting (Phase 3.4).
//!
//! Ijima is Schubert's first field deployment; this wiring exercises the
//! `RateLimiter` against real multi-tenant traffic and feeds back into
//! Schubert's development.
//!
//! ## How it composes
//!
//! Schubert's [`RateLimiter`](schubert::rate_limit::RateLimiter) gives each
//! principal a token bucket whose **capacity = intersection_number ×
//! multiplier × base_rate**. The intersection number is the **max
//! codimension across the grant's capabilities** — the token's
//! highest-privilege class: a `memory:write` grant (codim 2) gets 2× the
//! throughput of a `memory:read` grant (codim 1); an `admin` grant (point
//! class, codim 16) gets 16×. *The geometry of access maps to the geometry
//! of throughput.*
//!
//! Rate limiting is enforced inside the [`crate::extractor::AuthPrincipal`]
//! extractor — the capability token drives authentication, authorization,
//! **and** throughput in one step. A request that exhausts its bucket is
//! rejected with HTTP 429.
//!
//! ## Concurrency
//!
//! `RateLimiter::try_consume` is `&mut self`, so the limiter is wrapped in
//! a [`std::sync::Mutex`]. The critical section is sub-microsecond token
//! math; `std::sync::Mutex` (not async) is correct here because the hold is
//! too short to await and we never hold it across a `.await`.

use std::sync::{Arc, Mutex};

use schubert::PrincipalId;
use schubert::rate_limit::RateLimiter;

use crate::auth::AuthenticatedPrincipal;

/// Shared, thread-safe rate-limiter state.
pub type RateLimitState = Arc<Mutex<RateLimiter>>;

/// Constructs a fresh rate limiter from operator config.
///
/// - `base_tokens_per_second`: tokens granted per second per unit of
///   intersection number (env `IJIMA_RATE_BASE`, default `10`).
/// - `multiplier`: scales the intersection number (env
///   `IJIMA_RATE_MULTIPLIER`, default `1.0`). Use `< 1.0` to compress the
///   codimension spread (e.g. tame the 16× admin ratio).
pub fn make_rate_limiter(base_tokens_per_second: f64, multiplier: f64) -> RateLimitState {
    Arc::new(Mutex::new(RateLimiter::new(
        base_tokens_per_second,
        multiplier,
    )))
}

/// Consumes one rate-limit token for `principal`, configuring the bucket on
/// first sight from the capability's intersection number.
///
/// Returns `Ok(())` if a token was available, or the Schubert
/// `RateLimitExceeded` error (which the extractor maps to HTTP 429).
///
/// # Errors
///
/// Propagates [`schubert::SchubertError::RateLimitExceeded`] when the
/// principal's bucket is empty.
pub fn consume(
    state: &RateLimitState,
    principal: &AuthenticatedPrincipal,
) -> schubert::error::Result<()> {
    let mut rl = state.lock().expect("rate limiter poisoned");
    let pid: PrincipalId = principal.principal.clone();
    // Configure on first sight only — configure_principal resets the
    // bucket to full, so calling it every request would defeat limiting.
    // Weight = the max codimension across the grant's capabilities: a
    // token's rate-limit "weight class" reflects its highest-privilege
    // capability (admin [4,4,4,4] = 16, memory:write [2] = 2, read [1] = 1).
    if rl.capacity(pid.clone()).is_none() {
        let weight = principal
            .grant
            .capabilities
            .iter()
            .map(|c| c.partition.iter().sum::<usize>() as u64)
            .max()
            .unwrap_or(1);
        rl.configure_principal(pid.clone(), weight);
    }
    rl.try_consume(pid).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::IjimaAuth;
    use ijima_core::capabilities::{ADMIN, MEMORY_READ, MEMORY_WRITE};

    fn auth() -> IjimaAuth {
        IjimaAuth::from_embedded_policy().expect("policy must load")
    }

    /// Issue + verify a single-capability grant for `name`.
    fn principal(auth: &IjimaAuth, name: &str, cap: &str) -> AuthenticatedPrincipal {
        let bearer = auth.issue_bearer(name, cap).expect("issue");
        auth.verify_bearer(&bearer).expect("verify")
    }

    #[test]
    fn first_request_configures_and_consumes() {
        // base 1.0, multiplier 1.0 → memory:read (codim 1) gets capacity 1.
        let state = make_rate_limiter(1.0, 1.0);
        let auth = auth();
        let p = principal(&auth, "elliott", MEMORY_READ);
        assert!(consume(&state, &p).is_ok()); // 1 token, now 0
        assert!(consume(&state, &p).is_err()); // exhausted
    }

    #[test]
    fn higher_codimension_gets_more_capacity() {
        // memory:write (codim 2) at base 5 → capacity 10; memory:read
        // (codim 1) → capacity 5. Write holder survives twice as many.
        let state = make_rate_limiter(5.0, 1.0);
        let auth = auth();
        let reader = principal(&auth, "reader", MEMORY_READ);
        let writer = principal(&auth, "writer", MEMORY_WRITE);
        // exhaust reader in 5, writer in 10
        for _ in 0..5 {
            assert!(consume(&state, &reader).is_ok());
        }
        assert!(consume(&state, &reader).is_err());
        for _ in 0..10 {
            assert!(consume(&state, &writer).is_ok());
        }
        assert!(consume(&state, &writer).is_err());
    }

    #[test]
    fn admin_gets_point_class_capacity() {
        // admin (codim 16) at base 1, multiplier 1 → capacity 16.
        let state = make_rate_limiter(1.0, 1.0);
        let auth = auth();
        let admin = principal(&auth, "admin", ADMIN);
        for _ in 0..16 {
            assert!(consume(&state, &admin).is_ok());
        }
        assert!(consume(&state, &admin).is_err());
    }

    #[test]
    fn multiplier_compresses_admin_ratio() {
        // multiplier 0.1 → admin (codim 16) capacity = 16 * 0.1 * base.
        // At base 10: capacity 16, reader capacity 1.
        let state = make_rate_limiter(10.0, 0.1);
        let auth = auth();
        let reader = principal(&auth, "reader", MEMORY_READ);
        let admin = principal(&auth, "admin", ADMIN);
        // reader: codim 1 * 0.1 * 10 = 1 token
        assert!(consume(&state, &reader).is_ok());
        assert!(consume(&state, &reader).is_err());
        // admin: codim 16 * 0.1 * 10 = 16 tokens
        for _ in 0..16 {
            assert!(consume(&state, &admin).is_ok());
        }
        assert!(consume(&state, &admin).is_err());
    }

    #[test]
    fn multi_cap_grant_uses_max_codimension() {
        // A grant carrying memory:read (codim 1) + memory:write (codim 2)
        // is weighted by the max — 2 — so it gets write-class throughput
        // even though read is also present.
        let state = make_rate_limiter(5.0, 1.0);
        let auth = auth();
        let bearer = auth
            .issue_grant_bearer("pi", &[MEMORY_READ, MEMORY_WRITE])
            .expect("issue");
        let p = auth.verify_bearer(&bearer).expect("verify");
        // weight 2 * base 5 = capacity 10
        for _ in 0..10 {
            assert!(consume(&state, &p).is_ok());
        }
        assert!(consume(&state, &p).is_err());
    }
}
