//! Authenticated, read-only system snapshots. No room contents or credentials.
use super::*;
use std::{fmt::Write, time::Instant};
mod cluster;

pub(super) struct System {
    started: Instant,
    parallelism: usize,
    executable_bytes: Option<u64>,
}
impl System {
    pub(super) fn new() -> Self {
        Self {
            started: Instant::now(),
            parallelism: std::thread::available_parallelism().map_or(1, usize::from),
            executable_bytes: std::env::current_exe()
                .ok()
                .and_then(|path| std::fs::metadata(path).ok())
                .filter(|metadata| metadata.is_file())
                .map(|metadata| metadata.len()),
        }
    }
}

#[derive(Serialize)]
struct Memory {
    resident_bytes: Option<u64>,
    peak_resident_bytes: Option<u64>,
}

#[derive(Serialize)]
struct Status {
    schema_version: u8,
    version: &'static str,
    operating_system: &'static str,
    architecture: &'static str,
    process_id: u32,
    available_parallelism: usize,
    uptime_seconds: u64,
    executable_bytes: Option<u64>,
    memory: Memory,
    durability: &'static str,
    embedded_asset_bytes: usize,
    websocket_connections: usize,
    websocket_limit: usize,
    tcp_limit: usize,
    room_limit: usize,
    namespace_limit: usize,
    session_limit: usize,
    runtime: owner::PoolSnapshot,
    cluster: Option<peer::Observation>,
}
impl Status {
    fn read(app: &App) -> Self {
        let memory = morrow_web_app::system::process_memory();
        Self {
            schema_version: 1,
            version: env!("CARGO_PKG_VERSION"),
            operating_system: std::env::consts::OS,
            architecture: std::env::consts::ARCH,
            process_id: std::process::id(),
            available_parallelism: app.system.parallelism,
            uptime_seconds: app.system.started.elapsed().as_secs(),
            executable_bytes: app.system.executable_bytes,
            memory: Memory {
                resident_bytes: memory.resident_bytes,
                peak_resident_bytes: memory.peak_resident_bytes,
            },
            durability: if app.config.data_dir.is_some() {
                "checkpointed"
            } else {
                "ephemeral"
            },
            embedded_asset_bytes: app.assets.iter().map(|(_, bytes)| bytes.len()).sum(),
            websocket_connections: app
                .config
                .limits
                .max_connections
                .saturating_sub(app.sockets.available_permits()),
            websocket_limit: app.config.limits.max_connections,
            tcp_limit: app.config.max_tcp_connections,
            room_limit: app.config.limits.max_rooms,
            namespace_limit: app.config.limits.max_namespaces,
            session_limit: app.config.max_sessions,
            runtime: app.requests.snapshot(),
            cluster: app.cluster.as_ref().map(peer::Cluster::observe),
        }
    }
}

async fn authorize(app: &App, headers: &HeaderMap) -> Result<(), StatusCode> {
    if headers.contains_key(header::ORIGIN) {
        origin(headers, app)?;
    }
    let (reply, rx) = oneshot::channel();
    response(
        app,
        owner::Request::Authenticate {
            token: cookie(headers)?,
            csrf: None,
            reply,
        },
        rx,
    )
    .await?;
    Ok(())
}

fn private(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(
        "default-src 'none'; script-src 'none'; style-src 'self'; img-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'"));
    response
}

pub(super) async fn status(State(app): State<App>, headers: HeaderMap) -> Response {
    private(match authorize(&app, &headers).await {
        Ok(()) => Json(Status::read(&app)).into_response(),
        Err(code) => code.into_response(),
    })
}

pub(super) async fn page(State(app): State<App>, headers: HeaderMap) -> Response {
    private(match authorize(&app, &headers).await {
        Ok(()) => axum::response::Html(render(&Status::read(&app))).into_response(),
        Err(code) => {
            let content = match code {
                StatusCode::UNAUTHORIZED => {
                    "<h1>Sign in to see your system.</h1><p>The dashboard uses the application’s current session.</p><a class=button href=/>Open the application</a>"
                }
                StatusCode::FORBIDDEN => {
                    "<h1>This request is not allowed.</h1><p>Open the dashboard from the application’s own address.</p><a class=button href=/>Open the application</a>"
                }
                _ => {
                    "<h1>Snapshot temporarily unavailable.</h1><p>The server could not authorize this request within its current capacity.</p><a class=button href=/admin>Try again</a>"
                }
            };
            (
                code,
                axum::response::Html(shell(&format!(
                    "<main><p class=eyebrow>SERVER ACCESS</p>{content}</main>"
                ))),
            )
                .into_response()
        }
    })
}

pub(super) async fn stylesheet() -> Response {
    private(
        (
            [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
            include_str!("admin/style.css"),
        )
            .into_response(),
    )
}

fn shell(body: &str) -> String {
    format!(
        "<!doctype html><html lang=en><head><meta charset=utf-8><meta name=viewport content=\"width=device-width, initial-scale=1\"><meta name=theme-color content=\"#173e2e\"><title>Morrow system</title><link rel=stylesheet href=/admin/style.css></head><body><header><a class=brand href=/>Morrow</a><span class=eyebrow>SYSTEM OVERVIEW</span><a href=/>Back to garden</a></header>{body}</body></html>"
    )
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn bytes(value: usize) -> String {
    format!("{:.2} MiB", value as f64 / 1_048_576.0)
}

fn optional_bytes(value: Option<u64>) -> String {
    value.map_or_else(
        || "Unavailable".into(),
        |bytes| format!("{:.2} MiB", bytes as f64 / 1_048_576.0),
    )
}

fn render(status: &Status) -> String {
    let mut body = String::from(
        "<main><section class=intro><div><p class=eyebrow>ROOTED. RUNNING. OBSERVABLE.</p><h1>A little clarity<br>for your system.</h1><p>A snapshot of the server behind your shared garden.</p></div><nav aria-label=\"Dashboard actions\"><a class=button href=/admin>Refresh snapshot</a><a href=/admin/status>JSON status ↗</a></nav></section>",
    );
    let workers = &status.runtime.workers;
    let rooms: usize = workers.iter().map(|worker| worker.rooms).sum();
    let _ = write!(
        body,
        "<section class=metrics aria-label=\"System summary\"><article><p class=eyebrow>UPTIME</p><strong>{}h {:02}m {:02}s</strong><p>Since this server started</p></article><article><p class=eyebrow>ACTOR WORKERS</p><strong>{}</strong><p>Independently pinned threads</p></article><article><p class=eyebrow>OPEN WEBSOCKETS</p><strong>{}<small> / {}</small></strong><p>Includes sockets awaiting a room</p></article><article><p class=eyebrow>RESIDENT MEMORY</p><strong>{}</strong><p>Server process · OS snapshot</p></article></section>",
        status.uptime_seconds / 3600,
        status.uptime_seconds / 60 % 60,
        status.uptime_seconds % 60,
        workers.len(),
        status.websocket_connections,
        status.websocket_limit,
        optional_bytes(status.memory.resident_bytes)
    );
    cluster::render(&mut body, status.cluster.as_ref());
    body.push_str("<section class=panel><div class=section-heading><div><p class=eyebrow>ACTOR RUNTIME</p><h2>Workers, at a glance.</h2></div><span class=badge>Native Morrow</span></div><div class=table-scroll tabindex=0 role=region aria-label=\"Worker observations\"><table><caption>Counts from each worker’s last completed observation; busy work may change them.</caption><thead><tr><th scope=col>Worker</th><th scope=col>State</th><th scope=col>Rooms</th><th scope=col>Namespaces</th><th scope=col>Connections</th><th scope=col>Subscriptions</th></tr></thead><tbody>");
    for worker in workers {
        let state = match worker.state {
            owner::WorkerState::Idle => "Idle",
            owner::WorkerState::Busy => "Busy",
            owner::WorkerState::Stopped => "Stopped",
        };
        let _ = write!(
            body,
            "<tr><th scope=row>Worker {:02}</th><td><span class=badge>{state}</span></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            worker.index + 1,
            worker.rooms,
            worker.namespaces,
            worker.connections,
            worker.subscriptions
        );
    }
    body.push_str("</tbody></table></div></section><div class=details><section class=panel><p class=eyebrow>HOST &amp; BUILD</p><h2>One running binary.</h2><dl>");
    let executable = optional_bytes(status.executable_bytes);
    for (name, value) in [
        ("Morrow version", status.version.to_owned()),
        (
            "Platform",
            format!("{} · {}", status.operating_system, status.architecture),
        ),
        ("Process ID", status.process_id.to_string()),
        (
            "Available CPU parallelism",
            status.available_parallelism.to_string(),
        ),
        ("Executable size", executable),
        (
            "Resident memory (RSS)",
            optional_bytes(status.memory.resident_bytes),
        ),
        (
            "Peak resident memory",
            optional_bytes(status.memory.peak_resident_bytes),
        ),
        (
            "Embedded browser assets",
            bytes(status.embedded_asset_bytes),
        ),
    ] {
        let _ = write!(
            body,
            "<div><dt>{name}</dt><dd>{}</dd></div>",
            escape(&value)
        );
    }
    body.push_str("</dl></section><section class=panel><p class=eyebrow>CAPACITY &amp; RECOVERY</p><h2>Explicit boundaries.</h2><dl>");
    for (name, value) in [
        ("Room storage", status.durability.to_owned()),
        ("Loaded rooms", format!("{rooms} / {}", status.room_limit)),
        (
            "Retained sessions",
            format!(
                "{} / {}",
                status.runtime.authentication.retained_sessions, status.session_limit
            ),
        ),
        (
            "Authentication owner",
            if status.runtime.authentication.stopped {
                "Stopped"
            } else {
                "Running"
            }
            .into(),
        ),
        (
            "Ingress in use",
            format!(
                "{} / {}",
                status.runtime.ingress_in_use, status.runtime.ingress_limit
            ),
        ),
        ("TCP admission limit", status.tcp_limit.to_string()),
        ("Namespace limit", status.namespace_limit.to_string()),
    ] {
        let _ = write!(
            body,
            "<div><dt>{name}</dt><dd>{}</dd></div>",
            escape(&value)
        );
    }
    body.push_str("</dl></section></div><footer><p>Read-only snapshot · Refresh to observe changes.</p><p>The preview shares one access key for the application and dashboard. Counts are independent observations, not a globally atomic snapshot. No room contents or credentials are displayed.</p></footer></main>");
    shell(&body)
}
