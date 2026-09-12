//! A deployable preview server with browser assets embedded at compilation.
#![forbid(unsafe_code)]
use fern_web::{BoundedListener, Config, EMBEDDED_ASSETS, router};

#[tokio::main]
async fn main() -> std::process::ExitCode {
    if let Err(error) = run().await {
        eprintln!("fern-web: {error}");
        std::process::ExitCode::FAILURE
    } else {
        std::process::ExitCode::SUCCESS
    }
}
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--licenses"] {
        print!(
            "{}\n{}",
            include_str!("../../../LICENSE"),
            include_str!("../../../THIRD_PARTY_NOTICES.md")
        );
        return Ok(());
    }
    if args == ["--help"] || args == ["-h"] {
        println!(
            "fern-web: ephemeral collaborative Fern preview\n\nBuild: cargo xtask web-build\nRun: FERN_WEB_ACCESS_KEY=<at least 16 characters> fern-web\nLicenses: fern-web --licenses\n\nFERN_WEB_BIND defaults to 127.0.0.1:3000.\nFERN_WEB_ORIGIN is the exact public http(s) origin; required for non-loopback binds.\nBrowser files are embedded; no asset directory is required at runtime.\nThe preview uses ephemeral state and a shared access key. HTTPS requires a TLS terminator."
        );
        return Ok(());
    }
    if !args.is_empty() {
        return Err("unknown arguments; use --help".into());
    }
    if EMBEDDED_ASSETS.is_empty() {
        return Err("browser assets are not embedded; run cargo xtask web-build".into());
    }
    let access_key = std::env::var("FERN_WEB_ACCESS_KEY")
        .map_err(|_| "set FERN_WEB_ACCESS_KEY to a secret of at least 16 characters")?;
    let bind: std::net::SocketAddr = std::env::var("FERN_WEB_BIND")
        .unwrap_or_else(|_| "127.0.0.1:3000".into())
        .parse()?;
    let listener = tokio::net::TcpListener::bind(bind).await?;
    let address = listener.local_addr()?;
    let origin = match std::env::var("FERN_WEB_ORIGIN") {
        Ok(origin) => origin,
        Err(_) if address.ip().is_loopback() => format!("http://{address}"),
        Err(_) => return Err("set FERN_WEB_ORIGIN when binding outside loopback".into()),
    };
    let config = Config::new(origin.clone(), access_key);
    let listener = BoundedListener::new(
        listener,
        config.max_tcp_connections,
        config.handshake_timeout,
    )?;
    let app = router(config, EMBEDDED_ASSETS)?;
    eprintln!("Fern collaborative preview listening at {address}; browser origin {origin}");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
