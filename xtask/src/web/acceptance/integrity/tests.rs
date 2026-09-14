use super::*;
use std::{
    io::{Read, Write},
    net::TcpStream,
    time::Instant,
};

#[test]
fn idle_speculative_connection_does_not_block_an_active_asset_request() {
    let fixture = Fixture::new(BTreeMap::from([(
        "/index.html".into(),
        b"exact active document".to_vec(),
    )]))
    .unwrap();
    let address = fixture.origin.strip_prefix("http://").unwrap();
    // The idle socket is established first and remains owned/open until after the
    // active request completes. It represents a browser's speculative preconnect.
    let mut idle = TcpStream::connect(address).unwrap();
    idle.set_read_timeout(Some(Duration::from_millis(50)))
        .unwrap();
    let mut probe = [0; 1];
    assert!(
        matches!(idle.peek(&mut probe), Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut)),
        "fixture replied to or closed an incomplete speculative request"
    );
    let mut active = TcpStream::connect(address).unwrap();
    active
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    active
        .set_write_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    let started = Instant::now();
    active
        .write_all(b"GET / HTTP/1.1\r\nHost: fixture\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut response = Vec::new();
    active
        .read_to_end(&mut response)
        .expect("active request stalled behind idle socket");
    assert!(started.elapsed() < Duration::from_millis(500));
    assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));
    let body = response
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .unwrap()
        + 4;
    assert_eq!(&response[body..], b"exact active document");
    idle.set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    idle.write_all(b"GET / HTTP/1.1\r\nHost: fixture\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut delayed_response = Vec::new();
    idle.read_to_end(&mut delayed_response).unwrap();
    assert_eq!(delayed_response, response);
}

fn get(fixture: &Fixture, path: &str) -> (String, Vec<u8>) {
    let mut stream = TcpStream::connect(fixture.origin.strip_prefix("http://").unwrap()).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: fixture\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).unwrap();
    let body = bytes
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .unwrap()
        + 4;
    (
        String::from_utf8(bytes[..body].to_vec()).unwrap(),
        bytes[body..].to_vec(),
    )
}

#[test]
fn rolling_deployment_keeps_exact_http_success_tampering_and_mime() {
    let fixture = Fixture::new(BTreeMap::from([
        ("/worker.js".into(), b"original worker".to_vec()),
        ("/style.css".into(), b"original style".to_vec()),
        ("/morrow.wasm".into(), b"\0asm\x01\0\0\0".to_vec()),
    ]))
    .unwrap();
    assert_eq!(get(&fixture, "/worker.js").1, b"original worker");
    assert_eq!(get(&fixture, "/style.css").1, b"original style");
    fixture.changed.store(true, Ordering::Relaxed);
    let (headers, worker) = get(&fixture, "/worker.js");
    assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(headers.contains("Content-Type: text/javascript\r\n"));
    assert_eq!(
        worker,
        b"original worker\n// rolling deployment integrity regression\n"
    );
    let (headers, style) = get(&fixture, "/style.css");
    assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(headers.contains("Cache-Control: no-store\r\n"));
    assert_eq!(style, b"body { display: none !important; }");
    let (headers, wasm) = get(&fixture, "/morrow.wasm");
    assert!(headers.contains("Content-Type: application/wasm\r\n"));
    assert_eq!(wasm, b"\0asm\x01\0\0\0");
    assert!(
        get(&fixture, "/missing")
            .0
            .starts_with("HTTP/1.1 404 Not Found\r\n")
    );
    fixture.changed.store(false, Ordering::Relaxed);
    assert_eq!(get(&fixture, "/worker.js").1, b"original worker");
    assert_eq!(get(&fixture, "/style.css").1, b"original style");
}

#[test]
fn fragmented_headers_wait_and_fixture_shutdown_closes_retained_connections() {
    let fixture = Fixture::new(BTreeMap::from([(
        "/index.html".into(),
        b"complete document".to_vec(),
    )]))
    .unwrap();
    let address = fixture.origin.strip_prefix("http://").unwrap();
    let mut fragmented = TcpStream::connect(address).unwrap();
    fragmented
        .set_read_timeout(Some(Duration::from_millis(50)))
        .unwrap();
    fragmented.write_all(b"GET / HTTP/1.1\r\nHost:").unwrap();
    let mut probe = [0; 1];
    assert!(
        matches!(fragmented.peek(&mut probe), Err(error) if matches!(error.kind(),std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut))
    );
    assert_eq!(get(&fixture, "/").1, b"complete document");
    fragmented.write_all(b" fixture\r\n\r\n").unwrap();
    fragmented
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    let mut bytes = Vec::new();
    fragmented.read_to_end(&mut bytes).unwrap();
    assert!(bytes.ends_with(b"\r\n\r\ncomplete document"));
    let mut idle = TcpStream::connect(address).unwrap();
    idle.set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    let started = Instant::now();
    drop(fixture);
    assert!(started.elapsed() < Duration::from_millis(500));
    assert!(match idle.read(&mut probe) {
        Ok(0) => true,
        Err(error) => error.kind() == std::io::ErrorKind::ConnectionReset,
        _ => false,
    });
}
