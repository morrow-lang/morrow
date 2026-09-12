//! Bounded native transport for the ephemeral collaborative Fern preview.
#![forbid(unsafe_code)]
mod listener;
mod owner;
mod socket;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Request, State, WebSocketUpgrade},
    http::{HeaderMap, HeaderValue, StatusCode, Uri, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
pub use listener::BoundedListener;
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use tokio::sync::{Semaphore, mpsc, oneshot};

/// Explicit preview authentication and admission settings.
#[derive(Clone)]
pub struct Config {
    pub origin: String,
    pub access_key: String,
    pub max_sessions: usize,
    pub session_ttl: std::time::Duration,
    pub write_timeout: std::time::Duration,
    pub max_tcp_connections: usize,
    pub handshake_timeout: std::time::Duration,
    pub limits: fern_web_protocol::Limits,
}
impl Config {
    pub fn new(origin: String, access_key: String) -> Self {
        Self {
            origin,
            access_key,
            max_sessions: 256,
            session_ttl: std::time::Duration::from_secs(3600),
            write_timeout: std::time::Duration::from_secs(2),
            max_tcp_connections: 512,
            handshake_timeout: std::time::Duration::from_secs(10),
            limits: Default::default(),
        }
    }
}
/// Compile-time browser assets. An empty collection explicitly serves build instructions.
pub type Assets = &'static [(&'static str, &'static [u8])];
include!(concat!(env!("OUT_DIR"), "/assets.rs"));

#[derive(Clone)]
pub(crate) struct App {
    config: Arc<Config>,
    requests: mpsc::Sender<owner::Request>,
    sockets: Arc<Semaphore>,
    http: Arc<Semaphore>,
    assets: Assets,
}
/// Build a router inside a Tokio runtime. Serve it through [`BoundedListener`]
/// (as [`serve`] does) to enforce admission before HTTP parsing. Authentication
/// authorizes all preview rooms under one shared access key; applications need
/// their own resource policy.
pub fn router(config: Config, assets: Assets) -> Result<Router, std::io::Error> {
    let origin: Uri = config.origin.parse().map_err(std::io::Error::other)?;
    if !matches!(origin.scheme_str(), Some("http" | "https"))
        || origin.authority().is_none()
        || origin
            .path_and_query()
            .is_some_and(|path| path.as_str() != "/")
        || config.origin.ends_with('/')
        || config.origin.contains('@')
        || !(16..=256).contains(&config.access_key.len())
        || !(1..=1024).contains(&config.max_sessions)
        || config.session_ttl.is_zero()
        || config.session_ttl > Duration::from_secs(86_400)
        || config.write_timeout.is_zero()
        || config.write_timeout > Duration::from_secs(30)
        || !(1..=2048).contains(&config.max_tcp_connections)
        || config.handshake_timeout.is_zero()
        || config.handshake_timeout > Duration::from_secs(30)
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid origin, key or transport limits",
        ));
    }
    let sockets = Arc::new(Semaphore::new(config.limits.max_connections));
    let requests = owner::start(config.clone())?;
    let app = App {
        config: Arc::new(config),
        requests,
        sockets,
        http: Arc::new(Semaphore::new(512)),
        assets,
    };
    Ok(Router::new()
        .route("/session", get(session).post(login))
        .route("/logout", post(logout))
        .route("/ws", get(upgrade))
        .route("/health", get(|| async { "fern-web ephemeral preview\n" }))
        .fallback(asset)
        .layer(DefaultBodyLimit::max(1024))
        .layer(middleware::from_fn_with_state(app.clone(), admission))
        .with_state(app))
}

/// Serve embedded assets and the preview protocol with bounded TCP admission.
pub async fn serve(
    listener: tokio::net::TcpListener,
    config: Config,
    assets: Assets,
) -> std::io::Result<()> {
    let listener = BoundedListener::new(
        listener,
        config.max_tcp_connections,
        config.handshake_timeout,
    )?;
    axum::serve(listener, router(config, assets)?).await
}

async fn admission(State(app): State<App>, request: Request, next: Next) -> Response {
    let Ok(_permit) = app.http.clone().try_acquire_owned() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    match tokio::time::timeout(Duration::from_secs(5), next.run(request)).await {
        Ok(mut response) => {
            if response.status() != StatusCode::SWITCHING_PROTOCOLS {
                response
                    .headers_mut()
                    .insert(header::CONNECTION, HeaderValue::from_static("close"));
            }
            response.headers_mut().insert(
                header::X_CONTENT_TYPE_OPTIONS,
                HeaderValue::from_static("nosniff"),
            );
            response.headers_mut().insert(
                header::REFERRER_POLICY,
                HeaderValue::from_static("no-referrer"),
            );
            response
        }
        Err(_) => StatusCode::REQUEST_TIMEOUT.into_response(),
    }
}
fn origin(headers: &HeaderMap, app: &App) -> Result<(), StatusCode> {
    if headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) != Some(&app.config.origin) {
        Err(StatusCode::FORBIDDEN)
    } else {
        Ok(())
    }
}
fn cookie(headers: &HeaderMap) -> Result<String, StatusCode> {
    let values = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let mut found = values
        .split(';')
        .filter_map(|v| v.trim().strip_prefix("fern_session="));
    let token = found.next().ok_or(StatusCode::UNAUTHORIZED)?;
    if found.next().is_some() || token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(token.into())
}
async fn response<T>(
    app: &App,
    request: owner::Request,
    rx: oneshot::Receiver<Result<T, StatusCode>>,
) -> Result<T, StatusCode> {
    app.requests
        .try_send(request)
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    tokio::time::timeout(Duration::from_secs(2), rx)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Credentials {
    access_key: String,
}
#[derive(Serialize)]
struct SessionResponse {
    csrf: String,
}
fn session_response(authentication: owner::Authentication) -> Response {
    let mut response = Json(SessionResponse {
        csrf: authentication.csrf,
    })
    .into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
async fn login(
    State(app): State<App>,
    headers: HeaderMap,
    Json(credentials): Json<Credentials>,
) -> Result<Response, StatusCode> {
    origin(&headers, &app)?;
    let (reply, rx) = oneshot::channel();
    let auth = response(
        &app,
        owner::Request::Login {
            key: credentials.access_key,
            reply,
        },
        rx,
    )
    .await?;
    let cookie = format!(
        "fern_session={}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}{}",
        auth.token,
        app.config.session_ttl.as_secs().max(1),
        if app.config.origin.starts_with("https:") {
            "; Secure"
        } else {
            ""
        }
    );
    let mut response = session_response(auth);
    response.headers_mut().insert(
        header::SET_COOKIE,
        cookie
            .parse()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    );
    Ok(response)
}
async fn session(State(app): State<App>, headers: HeaderMap) -> Result<Response, StatusCode> {
    // Same-origin fetch may omit Origin for GET. No CORS response is provided;
    // cross-origin callers cannot read the nonce. Explicit foreign origins fail.
    if headers.contains_key(header::ORIGIN) {
        origin(&headers, &app)?;
    }
    let (reply, rx) = oneshot::channel();
    Ok(session_response(
        response(
            &app,
            owner::Request::Authenticate {
                token: cookie(&headers)?,
                csrf: None,
                reply,
            },
            rx,
        )
        .await?,
    ))
}
async fn logout(State(app): State<App>, headers: HeaderMap) -> Result<Response, StatusCode> {
    origin(&headers, &app)?;
    let csrf = headers
        .get("x-fern-csrf")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::FORBIDDEN)?
        .to_owned();
    let (reply, rx) = oneshot::channel();
    response(
        &app,
        owner::Request::Logout {
            token: cookie(&headers)?,
            csrf,
            reply,
        },
        rx,
    )
    .await?;
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_static("fern_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0"),
    );
    Ok(response)
}
async fn upgrade(
    State(app): State<App>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, StatusCode> {
    origin(&headers, &app)?;
    let protocols: Vec<_> = ws
        .requested_protocols()
        .filter_map(|v| v.to_str().ok())
        .collect();
    if !protocols.contains(&"fern.live.v1") {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut nonces = protocols
        .iter()
        .filter_map(|p| p.strip_prefix("fern.csrf."));
    let csrf = nonces.next().ok_or(StatusCode::FORBIDDEN)?.to_owned();
    if nonces.next().is_some() || csrf.len() != 64 {
        return Err(StatusCode::FORBIDDEN);
    }
    let permit = app
        .sockets
        .clone()
        .try_acquire_owned()
        .map_err(|_| StatusCode::TOO_MANY_REQUESTS)?;
    let (reply, rx) = oneshot::channel();
    let auth = response(
        &app,
        owner::Request::Authenticate {
            token: cookie(&headers)?,
            csrf: Some(csrf),
            reply,
        },
        rx,
    )
    .await?;
    Ok(ws
        .protocols(["fern.live.v1"])
        .max_message_size(fern_web_protocol::MAX_FRAME_BYTES)
        .max_frame_size(fern_web_protocol::MAX_FRAME_BYTES)
        .read_buffer_size(8192)
        .write_buffer_size(0)
        .max_write_buffer_size(2 * fern_web_protocol::MAX_FRAME_BYTES)
        .on_upgrade(move |socket| socket::run(socket, app, auth, permit)))
}
async fn asset(State(app): State<App>, uri: Uri) -> Response {
    let name = if uri.path() == "/" {
        "index.html"
    } else {
        uri.path().trim_start_matches('/')
    };
    let content_type = match name {
        "index.html" => "text/html; charset=utf-8",
        "bootstrap.js" | "fern_browser.js" | "worker.js" => "text/javascript; charset=utf-8",
        "fern_browser_bg.wasm" | "fern_app.wasm" => "application/wasm",
        "style.css" => "text/css; charset=utf-8",
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    let Some((_, bytes)) = app.assets.iter().find(|(path, _)| *path == name) else {
        return if name == "index.html" {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "Browser assets are not embedded. Run cargo xtask web-build.\n",
            )
                .into_response()
        } else {
            StatusCode::NOT_FOUND.into_response()
        };
    };
    let mut response = (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        *bytes,
    )
        .into_response();
    response.headers_mut().insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; connect-src 'self'; img-src 'self' data:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'"));
    if name == "worker.js" {
        response
            .headers_mut()
            .insert("service-worker-allowed", HeaderValue::from_static("/"));
    }
    response
}
