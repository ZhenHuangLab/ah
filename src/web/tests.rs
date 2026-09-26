use std::fs;
use std::os::unix::fs::symlink;

use axum::body::{Body, to_bytes};
use axum::http::Method;
use futures_util::StreamExt;
use tower::ServiceExt;

use super::*;
use crate::auth::{self, COOKIE, SESSION_SECS};
use crate::model::Agent;

/// Real handlers and parsers, with synthetic sessions and no process-wide environment changes.
struct Fixture {
    dir: PathBuf,
    state: Shared,
}

impl Fixture {
    fn new() -> Self {
        let mut random = [0; 16];
        getrandom::fill(&mut random).unwrap();
        let dir = std::env::temp_dir().join(format!("ah-security-{:032x}", u128::from_ne_bytes(random)));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("session.jsonl");
        let record = json!({
            "type": "user",
            "message": { "content": [
                { "type": "text", "text": "Synthetic private prompt" },
                { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "cHJpdmF0ZSBpbWFnZQ==" } }
            ] }
        });
        fs::write(&path, format!("{record}\n")).unwrap();
        let meta = SessionMeta {
            id: "test-session".into(),
            agent: Agent::Claude,
            path: path.clone(),
            cwd: "/private/project".into(),
            title: "Synthetic session".into(),
            started: 0,
            modified: 0,
            size: 0,
        };
        let live = Live::open(meta.clone()).unwrap();
        let mut index = Index::default();
        index.insert(meta);
        let (tx, _) = broadcast::channel(16);
        let state = Arc::new(AppState {
            roots: Roots { claude: None, codex: Vec::new(), pi: None },
            index: RwLock::new(index),
            open: Mutex::new(HashMap::from([(
                "test-session".into(),
                Open { live: Arc::new(Mutex::new(live)), path, used: Instant::now() },
            )])),
            tx,
            public: Some(Arc::new(Access { host: "ah.example.test".into(), key: auth::tests::key() })),
        });
        Self { dir, state }
    }

    fn apps(&self) -> (Router, Router) {
        let (local, public) = routers(self.state.clone());
        (local, public.unwrap())
    }

    fn session(&self) -> String {
        format!("{COOKIE}={}", self.state.public.as_ref().unwrap().key.session())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.dir).unwrap();
    }
}

fn request(method: Method, path: &str, cookie: Option<&str>, body: &str) -> Request {
    let mut req = Request::builder().method(method).uri(path).header(header::HOST, "localhost");
    if let Some(cookie) = cookie {
        req = req.header(header::COOKIE, cookie);
    }
    if !body.is_empty() {
        req = req.header(header::CONTENT_TYPE, "application/json");
    }
    req.body(Body::from(body.to_string())).unwrap()
}

fn assert_no_store(response: &Response) {
    assert_eq!(response.headers()[header::CACHE_CONTROL], "private, no-store");
}

#[tokio::test]
async fn anonymous_requests_cannot_reach_owner_routes() {
    let fixture = Fixture::new();
    let (_, public) = fixture.apps();
    for path in [
        "/",
        "/api/sessions",
        "/api/events",
        "/api/s/test-session",
        "/api/s/test-session/events?gen=0&rev=0",
        "/api/s/test-session/block/0/0",
        "/api/s/test-session/run/0/0",
        "/api/s/test-session/img/0/1",
        "/api/s/test-session/shares",
        "/api/login-link",
    ] {
        let response = public.clone().oneshot(request(Method::GET, path, None, "")).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
        assert_no_store(&response);
    }
    for (method, path, body) in [
        (Method::POST, "/api/s/test-session/shares", r#"{"view":"chat","days":7}"#),
        (Method::DELETE, "/api/shares/AAAAAAAAAAAAAAAAAAAAAA", ""),
    ] {
        let response = public.clone().oneshot(request(method, path, None, body)).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    for cookie in ["ah=9999999999.invalid", "other=valid", "ah-extra=valid"] {
        let response = public.clone().oneshot(request(Method::GET, "/api/sessions", Some(cookie), "")).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn private_responses_are_not_cacheable_on_either_listener() {
    let fixture = Fixture::new();
    let (local, public) = fixture.apps();
    let cookie = fixture.session();
    for (app, cookie) in [(local, None), (public, Some(cookie.as_str()))] {
        for path in ["/", "/api/sessions", "/api/s/test-session", "/api/s/test-session/img/0/1", "/api/login-link"] {
            let response = app.clone().oneshot(request(Method::GET, path, cookie, "")).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            assert_no_store(&response);
            let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
            if path.ends_with("/img/0/1") {
                assert_eq!(bytes.as_ref(), b"private image");
            }
        }
    }
}

#[tokio::test]
async fn login_preview_does_not_authenticate_and_post_sets_a_protected_cookie() {
    let fixture = Fixture::new();
    let (_, public) = fixture.apps();
    let access = fixture.state.public.as_ref().unwrap();
    let link = access.key.login_link(&access.host);
    let (_, token) = link.split_once("?t=").unwrap();
    let response = public.clone().oneshot(request(Method::GET, &format!("/login?t={token}"), None, "")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_no_store(&response);
    assert!(!response.headers().contains_key(header::SET_COOKIE));

    let req = Request::builder()
        .method(Method::POST)
        .uri("/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!("t={token}")))
        .unwrap();
    let response = public.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_no_store(&response);
    assert_eq!(response.headers()[header::LOCATION], "/");
    let set_cookie = response.headers()[header::SET_COOKIE].to_str().unwrap();
    for flag in ["HttpOnly", "Secure", "SameSite=Lax", "Path=/"] {
        assert!(set_cookie.split(';').any(|part| part.trim() == flag));
    }
    let cookie = set_cookie.split(';').next().unwrap();
    let response = public.clone().oneshot(request(Method::GET, "/api/sessions", Some(cookie), "")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = public.oneshot(request(Method::GET, "/api/sessions", Some(&format!("{COOKIE}={token}")), "")).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn missing_share_and_login_errors_cannot_be_cached() {
    let fixture = Fixture::new();
    let (_, public) = fixture.apps();
    for (path, status) in
        [("/api/share/invalid", StatusCode::NOT_FOUND), ("/s/invalid", StatusCode::NOT_FOUND), ("/login?t=invalid", StatusCode::FORBIDDEN)]
    {
        let response = public.clone().oneshot(request(Method::GET, path, None, "")).await.unwrap();
        assert_eq!(response.status(), status);
        assert_no_store(&response);
    }
}

#[tokio::test]
async fn assets_cannot_read_an_outside_symlink_even_in_debug_builds() {
    let fixture = Fixture::new();
    let (_, public) = fixture.apps();
    let secret = fixture.dir.join("secret.txt");
    let link = fixture.dir.join("link.txt");
    fs::write(&secret, "private data outside assets").unwrap();
    symlink(&secret, &link).unwrap();
    let absolute = link.to_str().unwrap().replace('/', "%2F");
    let traversal = format!("../../../../../../../{}", link.strip_prefix("/").unwrap().display()).replace('/', "%2F");
    let backslashes = link.to_str().unwrap().replace('/', "%5C");
    for path in [absolute, traversal, backslashes] {
        let response = public.clone().oneshot(request(Method::GET, &format!("/assets/{path}"), None, "")).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    let response = public.oneshot(request(Method::GET, "/assets/vendor/katex/katex.min.js", None, "")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "public, max-age=604800");
}

#[tokio::test(start_paused = true)]
async fn both_public_event_streams_stop_when_the_session_expires() {
    let fixture = Fixture::new();
    let (_, public) = fixture.apps();
    let cookie = fixture.session();
    let mut streams = Vec::new();
    for path in ["/api/events", "/api/s/test-session/events?gen=0&rev=0"] {
        let response = public.clone().oneshot(request(Method::GET, path, Some(&cookie), "")).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_no_store(&response);
        streams.push(response.into_body().into_data_stream());
    }
    assert!(fixture.state.tx.send(Change::Gone("before-expiry".into())).is_ok());
    let first = streams[0].next().await.unwrap().unwrap();
    assert!(std::str::from_utf8(&first).unwrap().contains("before-expiry"));
    assert!(streams[1].next().await.unwrap().is_ok());

    tokio::time::advance(Duration::from_secs(SESSION_SECS as u64 + 1)).await;
    assert!(fixture.state.tx.send(Change::Gone("after-expiry".into())).is_ok());
    assert!(fixture.state.tx.send(Change::Live("test-session".into())).is_ok());
    for stream in &mut streams {
        assert!(stream.next().await.is_none());
    }
    drop(streams);
    assert_eq!(fixture.state.tx.receiver_count(), 0);
}

#[tokio::test(start_paused = true)]
async fn a_request_cannot_finish_after_its_session_expires() {
    let access = Arc::new(Access { host: "ah.example.test".into(), key: auth::tests::key() });
    let cookie = format!("{COOKIE}={}", access.key.session());
    let app = Router::new()
        .route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(SESSION_SECS as u64 + 1)).await;
                "private result"
            }),
        )
        .route_layer(middleware::from_fn_with_state(access, public::signed_in));
    let response = app.oneshot(request(Method::GET, "/slow", Some(&cookie), "")).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
