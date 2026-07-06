// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! Axum extractor that authenticates requests via a Schubert capability
//! token in the `Authorization: Bearer <token>` header.
//!
//! Requires both `http` and `server-auth`. The [`IjimaAuth`] is shared
//! via axum's built-in [`Extension`] layer
//! (`router.layer(Extension(Arc::new(ijima_auth)))`). The handler
//! obtains an [`AuthenticatedPrincipal`] as a parameter; deeper
//! capability checks go through [`crate::IjimaAuth::require`] on the
//! same shared state.

use async_trait::async_trait;
use axum::{
    Extension,
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
    response::IntoResponse,
};
use std::sync::Arc;

use crate::auth::{AuthenticatedPrincipal, IjimaAuth};

/// Extractor: validates the bearer token and yields the authenticated
/// principal + capability.
///
/// ```ignore
/// async fn handler(auth: AuthPrincipal) -> impl IntoResponse {
///     format!("hello {}", auth.0.principal)
/// }
/// ```
#[derive(Debug, Clone)]
pub struct AuthPrincipal(pub AuthenticatedPrincipal);

/// Error returned when authentication fails; maps to HTTP 401.
#[derive(Debug)]
pub struct AuthError(pub &'static str);

impl IntoResponse for AuthError {
    fn into_response(self) -> axum::response::Response {
        (StatusCode::UNAUTHORIZED, self.0).into_response()
    }
}

#[async_trait]
impl<S> FromRequestParts<S> for AuthPrincipal
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let Extension(auth): Extension<Arc<IjimaAuth>> =
            Extension::from_request_parts(parts, _state)
                .await
                .map_err(|_| AuthError("auth state not installed"))?;

        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .ok_or(AuthError("missing Authorization header"))?;

        let token = header
            .strip_prefix("Bearer ")
            .ok_or(AuthError("expected 'Bearer <token>'"))?;

        let principal = auth
            .verify_bearer(token)
            .map_err(|_| AuthError("invalid capability token"))?;

        Ok(AuthPrincipal(principal))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ijima_core::capabilities::MEMORY_READ;

    fn build_request(bearer: Option<String>) -> (Parts, ()) {
        let mut builder = axum::http::Request::<()>::builder();
        if let Some(b) = bearer {
            builder = builder.header("authorization", format!("Bearer {b}"));
        }
        let req = builder.body(()).unwrap();
        req.into_parts()
    }

    #[tokio::test]
    async fn valid_bearer_header_yields_principal() {
        let auth = Arc::new(IjimaAuth::from_embedded_policy().expect("policy must load"));
        let bearer = auth.issue_bearer("elliott", MEMORY_READ).expect("issue");

        let (mut parts, ()) = build_request(Some(bearer));
        parts.extensions.insert(auth.clone());

        let state: () = ();
        let got = AuthPrincipal::from_request_parts(&mut parts, &state)
            .await
            .expect("must extract");
        assert_eq!(got.0.principal.as_str(), "elliott");
        assert_eq!(got.0.capability, MEMORY_READ);
    }

    #[tokio::test]
    async fn missing_header_is_401() {
        let auth = Arc::new(IjimaAuth::from_embedded_policy().expect("policy must load"));
        let (mut parts, ()) = build_request(None);
        parts.extensions.insert(auth);

        let state: () = ();
        let err = AuthPrincipal::from_request_parts(&mut parts, &state)
            .await
            .expect_err("must reject");
        assert_eq!(err.into_response().status(), StatusCode::UNAUTHORIZED);
    }
}
