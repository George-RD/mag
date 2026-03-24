use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::http::{Method, Request, Response, StatusCode};

/// Constant-time string comparison to prevent timing attacks on token validation.
fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// Tower layer that validates `Authorization: Bearer <token>` on incoming requests.
/// `GET /health` is exempt from authentication.
#[derive(Clone)]
pub struct AuthLayer {
    expected_token: Arc<String>,
}

impl AuthLayer {
    pub fn new(token: String) -> Self {
        Self {
            expected_token: Arc::new(token),
        }
    }
}

impl<S> tower::Layer<S> for AuthLayer {
    type Service = AuthService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthService {
            inner,
            expected_token: Arc::clone(&self.expected_token),
        }
    }
}

/// Tower service produced by [`AuthLayer`] that performs Bearer token validation.
#[derive(Clone)]
pub struct AuthService<S> {
    inner: S,
    expected_token: Arc<String>,
}

impl<S, B> tower::Service<Request<B>> for AuthService<S>
where
    S: tower::Service<Request<B>, Response = Response<Body>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    B: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        if req.method() == Method::GET && req.uri().path() == "/health" {
            let future = self.inner.call(req);
            return Box::pin(future);
        }

        let authorized = req
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .is_some_and(|token| constant_time_eq(token, &self.expected_token));

        if authorized {
            let future = self.inner.call(req);
            Box::pin(future)
        } else {
            Box::pin(async {
                Ok(Response::builder()
                    .status(StatusCode::UNAUTHORIZED)
                    .body(Body::from("Unauthorized"))
                    // Status + body-from-string is infallible; unwrap_or provides a fallback
                    .unwrap_or_else(|_| {
                        Response::new(Body::from("Unauthorized"))
                    }))
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use tower::{Service, ServiceBuilder, ServiceExt};

    /// Build a trivial handler wrapped in AuthLayer for testing.
    fn build_service(
        token: &str,
    ) -> impl Service<
        Request<Body>,
        Response = Response<Body>,
        Error = std::convert::Infallible,
        Future = impl Future<Output = Result<Response<Body>, std::convert::Infallible>> + Send,
    > + Clone {
        let handler = tower::service_fn(|_req: Request<Body>| async {
            Ok::<_, std::convert::Infallible>(
                Response::builder()
                    .status(StatusCode::OK)
                    .body(Body::from("ok"))
                    .expect("building static response cannot fail"),
            )
        });

        ServiceBuilder::new()
            .layer(AuthLayer::new(token.to_owned()))
            .service(handler)
    }

    #[tokio::test]
    async fn valid_bearer_token_passes() {
        let mut svc = build_service("secret-token");
        let req = Request::builder()
            .uri("/api/data")
            .header("Authorization", "Bearer secret-token")
            .body(Body::empty())
            .expect("building test request cannot fail");

        let resp = svc.ready().await.expect("service ready").call(req).await.expect("call succeeds");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn wrong_token_returns_401() {
        let mut svc = build_service("secret-token");
        let req = Request::builder()
            .uri("/api/data")
            .header("Authorization", "Bearer wrong-token")
            .body(Body::empty())
            .expect("building test request cannot fail");

        let resp = svc.ready().await.expect("service ready").call(req).await.expect("call succeeds");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn missing_auth_header_returns_401() {
        let mut svc = build_service("secret-token");
        let req = Request::builder()
            .uri("/api/data")
            .body(Body::empty())
            .expect("building test request cannot fail");

        let resp = svc.ready().await.expect("service ready").call(req).await.expect("call succeeds");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn get_health_bypasses_auth() {
        let mut svc = build_service("secret-token");
        let req = Request::builder()
            .method(Method::GET)
            .uri("/health")
            .body(Body::empty())
            .expect("building test request cannot fail");

        let resp = svc.ready().await.expect("service ready").call(req).await.expect("call succeeds");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn post_health_requires_auth() {
        let mut svc = build_service("secret-token");
        let req = Request::builder()
            .method(Method::POST)
            .uri("/health")
            .body(Body::empty())
            .expect("building test request cannot fail");

        let resp = svc.ready().await.expect("service ready").call(req).await.expect("call succeeds");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn constant_time_eq_equal_strings() {
        assert!(constant_time_eq("hello", "hello"));
    }

    #[test]
    fn constant_time_eq_different_strings() {
        assert!(!constant_time_eq("hello", "world"));
    }

    #[test]
    fn constant_time_eq_different_lengths() {
        assert!(!constant_time_eq("short", "longer-string"));
    }

    #[test]
    fn constant_time_eq_empty_strings() {
        assert!(constant_time_eq("", ""));
    }
}
