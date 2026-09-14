//! A deployable preview server with browser assets embedded at compilation.
#![forbid(unsafe_code)]
use morrow_web::{BoundedListener, Config, EMBEDDED_ASSETS, router};

#[tokio::main]
async fn main() -> std::process::ExitCode {
    if let Err(error) = run().await {
        eprintln!("morrow-web: {error}");
        std::process::ExitCode::FAILURE
    } else {
        std::process::ExitCode::SUCCESS
    }
}
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // At most three provisioning arguments plus sixteen member records.
    let args: Vec<_> = std::env::args_os().skip(1).take(20).collect();
    if args.len() > 19 || args.iter().any(|arg| arg.as_encoded_bytes().len() > 4096) {
        return Err("too many or oversized arguments; use --help".into());
    }
    let args: Vec<String> = args
        .into_iter()
        .map(|arg| arg.into_string().map_err(|_| "arguments must be UTF-8"))
        .collect::<Result<_, _>>()?;
    if args.first().is_some_and(|arg| arg == "--cluster-init") {
        return initialize_cluster(&args[1..]);
    }
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
            "morrow-web: compiled Morrow collaborative application\n\nBuild: cargo xtask web-build\nRun: MORROW_WEB_ACCESS_KEY=<at least 16 characters> morrow-web\nLicenses: morrow-web --licenses\nCluster: morrow-web --cluster-init <NEW_DIR> <CLUSTER_ID> <NODE=IP:PEERPORT>...\n\nMORROW_WEB_BIND defaults to 127.0.0.1:3000.\nMORROW_WEB_ORIGIN is the exact public http(s) origin; required for non-loopback binds.\nBrowser files are embedded; no asset directory is required at runtime.\nMORROW_WEB_WORKERS selects 1–32 pinned actor workers; default is CPU count capped at 4.\nMORROW_WEB_DATA_DIR enables durable room checkpoints; omit it for ephemeral state.\nMORROW_WEB_CLUSTER selects a generated node.json for authenticated server connections.\nCluster init accepts 1–16 nodes and requires a new directory inside a trusted parent.\nEach node uses its own bundle, browser bind/origin, access key and data directory.\nAuthentication and command namespaces restart with the server. HTTPS requires a TLS terminator."
        );
        return Ok(());
    }
    if !args.is_empty() {
        return Err("unknown arguments; use --help".into());
    }
    if EMBEDDED_ASSETS.is_empty() {
        return Err("browser assets are not embedded; run cargo xtask web-build".into());
    }
    let access_key = std::env::var("MORROW_WEB_ACCESS_KEY")
        .map_err(|_| "set MORROW_WEB_ACCESS_KEY to a secret of at least 16 characters")?;
    let bind: std::net::SocketAddr = std::env::var("MORROW_WEB_BIND")
        .unwrap_or_else(|_| "127.0.0.1:3000".into())
        .parse()?;
    let listener = tokio::net::TcpListener::bind(bind).await?;
    let address = listener.local_addr()?;
    let origin = match std::env::var("MORROW_WEB_ORIGIN") {
        Ok(origin) => origin,
        Err(_) if address.ip().is_loopback() => format!("http://{address}"),
        Err(_) => return Err("set MORROW_WEB_ORIGIN when binding outside loopback".into()),
    };
    let mut config = Config::new(origin.clone(), access_key);
    if let Some(workers) = std::env::var_os("MORROW_WEB_WORKERS") {
        config.workers = workers
            .to_str()
            .ok_or("MORROW_WEB_WORKERS must be an integer")?
            .parse()?;
    }
    config.data_dir = std::env::var_os("MORROW_WEB_DATA_DIR").map(std::path::PathBuf::from);
    config.cluster = std::env::var_os("MORROW_WEB_CLUSTER")
        .map(|path| morrow_cluster::NodeSettings::load(std::path::Path::new(&path)))
        .transpose()?;
    let listener = BoundedListener::new(
        listener,
        config.max_tcp_connections,
        config.handshake_timeout,
    )?;
    let app = router(config, EMBEDDED_ASSETS)?;
    eprintln!("Morrow collaborative preview listening at {address}; browser origin {origin}");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

fn initialize_cluster(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if !(3..=18).contains(&args.len()) {
        return Err("usage: morrow-web --cluster-init <NEW_DIR> <CLUSTER_ID> <NODE=IP:PEERPORT>... (1–16 nodes)".into());
    }
    let cluster = morrow_cluster::ClusterId::new(args[1].clone())?;
    let nodes = args[2..]
        .iter()
        .map(|argument| {
            let (node, address) = argument
                .split_once('=')
                .ok_or("member must be NODE=IP:PEERPORT")?;
            Ok((
                morrow_cluster::NodeId::new(node)?,
                address.parse::<std::net::SocketAddr>()?,
            ))
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    let provisioned = morrow_cluster::provision(std::path::Path::new(&args[0]), cluster, nodes)?;
    for node in provisioned.nodes {
        println!("Node {}: {}", node.node.as_str(), node.settings.display());
        println!(
            "  MORROW_WEB_CLUSTER={} MORROW_WEB_ACCESS_KEY='<at least 16 characters>' morrow-web",
            shell_quote(&node.settings.to_string_lossy())
        );
    }
    println!(
        "Copy each node's own bundle to its server. Set its browser bind/origin and a separate data directory before running."
    );
    Ok(())
}
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}
