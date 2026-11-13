//! Routes, plus the guard that every request passes first.

use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderName, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;

use super::auth;
use crate::runtime::Handle;

const INDEX_HTML: &str = include_str!("assets/index.html");
const APP_CSS: &str = include_str!("assets/app.css");
const APP_JS: &str = include_str!("assets/app.js");

#[derive(Clone)]
pub struct AppState {
    pub handle: Handle,
    pub token: Arc<str>,
    pub port: u16,
    pub next_client: Arc<AtomicU64>,
}

impl AppState {
    pub fn new(handle: Handle, token: Arc<str>, port: u16) -> Self {
        AppState {
            handle,
            token,
            port,
            next_client: Arc::new(AtomicU64::new(1)),
        }
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(|| async { Html(INDEX_HTML) }))
        .route(
            "/app.css",
            get(|| async { ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], APP_CSS) }),
        )
        .route(
            "/app.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    APP_JS,
                )
            }),
        )
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state)
}

enum Verdict {
    Pass,
    Login,
    Unauthorized,
    Forbidden(&'static str),
}

fn header_str(headers: &HeaderMap, name: HeaderName) -> Option<&str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

fn verdict(request: &Request, state: &AppState) -> Verdict {
    let headers = request.headers();
    if !auth::host_allowed(header_str(headers, header::HOST), state.port) {
        return Verdict::Forbidden("forbidden host");
    }
    let path = request.uri().path();
    if path == "/"
        && let Some(token) = request.uri().query().and_then(auth::query_token)
    {
        return if auth::token_matches(token, &state.token) {
            Verdict::Login
        } else {
            Verdict::Unauthorized
        };
    }
    let has_cookie = auth::cookie_token(header_str(headers, header::COOKIE))
        .is_some_and(|token| auth::token_matches(token, &state.token));
    if !has_cookie {
        return Verdict::Unauthorized;
    }
    if path == "/ws" && !auth::origin_allowed(header_str(headers, header::ORIGIN), state.port) {
        return Verdict::Forbidden("forbidden origin");
    }
    Verdict::Pass
}

async fn guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    match verdict(&request, &state) {
        Verdict::Pass => next.run(request).await,
        // Swapping the URL token for a cookie takes it out of the address bar and history.
        Verdict::Login => {
            let cookie = format!(
                "{}={}; HttpOnly; SameSite=Strict; Path=/",
                auth::COOKIE,
                state.token
            );
            (
                StatusCode::SEE_OTHER,
                [
                    (header::LOCATION, "/".to_string()),
                    (header::SET_COOKIE, cookie),
                ],
            )
                .into_response()
        }
        Verdict::Unauthorized => (
            StatusCode::UNAUTHORIZED,
            "Open the dashboard from shipherd's terminal UI (press w).",
        )
            .into_response(),
        Verdict::Forbidden(reason) => (StatusCode::FORBIDDEN, reason).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime;
    use crate::tty7::fake::FakeCli;
    use axum::body::{Body, to_bytes};
    use axum::http::Request as HttpRequest;
    use tower::ServiceExt;

    const PORT: u16 = 7777;
    const TOKEN: &str = "test-token-0123456789";

    fn app() -> Router {
        let handle = runtime::start(Arc::new(FakeCli::default()));
        router(AppState::new(handle, TOKEN.into(), PORT))
    }

    fn get(path: &str) -> axum::http::request::Builder {
        HttpRequest::get(path).header(header::HOST, format!("127.0.0.1:{PORT}"))
    }

    fn with_cookie(path: &str) -> axum::http::request::Builder {
        get(path).header(header::COOKIE, format!("shipherd={TOKEN}"))
    }

    async fn send(request: axum::http::request::Builder) -> Response {
        app()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn rejects_requests_without_a_token() {
        assert_eq!(send(get("/")).await.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            send(get("/app.js")).await.status(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn rejects_a_wrong_token() {
        assert_eq!(
            send(get("/?token=nope")).await.status(),
            StatusCode::UNAUTHORIZED
        );
        let wrong_cookie = get("/").header(header::COOKIE, "shipherd=nope");
        assert_eq!(send(wrong_cookie).await.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn login_link_sets_a_strict_http_only_cookie_and_redirects() {
        let res = send(get(&format!("/?token={TOKEN}"))).await;
        assert_eq!(res.status(), StatusCode::SEE_OTHER);
        assert_eq!(res.headers()[header::LOCATION], "/");
        assert_eq!(
            res.headers()[header::SET_COOKIE].to_str().unwrap(),
            format!("shipherd={TOKEN}; HttpOnly; SameSite=Strict; Path=/")
        );
    }

    #[tokio::test]
    async fn cookie_grants_access_to_the_page_and_assets() {
        let res = send(with_cookie("/")).await;
        assert_eq!(res.status(), StatusCode::OK);
        let body = to_bytes(res.into_body(), 1 << 20).await.unwrap();
        assert!(String::from_utf8_lossy(&body).contains("shipherd"));

        let res = send(with_cookie("/app.js")).await;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(
            res.headers()[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("text/javascript")
        );
    }

    #[tokio::test]
    async fn foreign_host_is_rejected_even_with_the_cookie() {
        let request = HttpRequest::get("/")
            .header(header::HOST, format!("evil.example:{PORT}"))
            .header(header::COOKIE, format!("shipherd={TOKEN}"));
        assert_eq!(send(request).await.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn responses_carry_no_cors_headers() {
        let res = send(with_cookie("/").header(header::ORIGIN, "https://evil.example")).await;
        assert!(
            res.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .is_none()
        );
    }
}
