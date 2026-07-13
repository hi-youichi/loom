//! Authorization observability middleware (task P0.7).
//!
//! External mode clients may send Basic or Bearer credentials. The rollout
//! intentionally keeps authentication permissive, but records whether a
//! header was present without ever logging its credential bytes.

use axum::{extract::Request, middleware::Next, response::Response};

pub const AUTHORIZATION: &str = "authorization";

pub async fn log_authorization_header(req: Request, next: Next) -> Response {
    let authorization = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    let method = req.method().clone();
    let uri = req.uri().clone();

    match authorization {
        Some(value) if !value.is_empty() => {
            tracing::debug!(
                method = %method,
                uri = %uri,
                scheme = authorization_scheme(value),
                header_len = value.len(),
                "authorization header present (accepted by rollout policy)"
            );
        }
        Some(_) => tracing::debug!(method = %method, uri = %uri, "authorization header empty"),
        None => tracing::trace!(method = %method, uri = %uri, "authorization header absent"),
    }

    next.run(req).await
}

fn authorization_scheme(value: &str) -> &str {
    value.split_ascii_whitespace().next().unwrap_or("unknown")
}

#[cfg(test)]
mod tests {
    use super::authorization_scheme;

    #[test]
    fn extracts_only_the_authorization_scheme() {
        assert_eq!(authorization_scheme("Basic dXNlcjpwYXNz"), "Basic");
        assert_eq!(authorization_scheme("Bearer secret"), "Bearer");
        assert_eq!(authorization_scheme(""), "unknown");
    }
}
