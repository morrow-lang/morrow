//! A rolling deployment must never install HTTP-successful bytes from another revision.
#[cfg(test)]
mod tests;
use super::{Browser, Result};
use base64::Engine as _;
use serde_json::json;
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

struct Fixture {
    origin: String,
    changed: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Fixture {
    fn new(assets: BTreeMap<String, Vec<u8>>) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        let origin = format!(
            "http://{}",
            listener.local_addr().map_err(|e| e.to_string())?
        );
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let changed = Arc::new(AtomicBool::new(false));
        let stopped = Arc::new(AtomicBool::new(false));
        let deployment = changed.clone();
        let stop = stopped.clone();
        let assets: BTreeMap<_, Arc<[u8]>> = assets
            .into_iter()
            .map(|(path, bytes)| (path, bytes.into()))
            .collect();
        let thread = thread::spawn(move || {
            let mut connections = Vec::with_capacity(MAX_CONNECTIONS);
            while !stop.load(Ordering::Relaxed) {
                // Admission and per-connection I/O work are bounded per turn.
                for _ in 0..MAX_CONNECTIONS {
                    match listener.accept() {
                        Ok((stream, _)) if connections.len() < MAX_CONNECTIONS => {
                            if stream.set_nonblocking(true).is_ok() {
                                connections.push(Connection::new(stream));
                            }
                        }
                        Ok(_) => {} // Close excess connections without creating another task.
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(_) => break,
                    }
                }
                connections.retain_mut(|connection| {
                    connection.advance(&assets, deployment.load(Ordering::Relaxed))
                });
                thread::sleep(Duration::from_millis(5));
            }
        });
        Ok(Self {
            origin,
            changed,
            stopped,
            thread: Some(thread),
        })
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

const MAX_CONNECTIONS: usize = 32;
const MAX_HEADER_BYTES: usize = 16 * 1024;
const WRITE_BYTES_PER_TURN: usize = 64 * 1024;

struct Connection {
    stream: TcpStream,
    deadline: Instant,
    state: State,
}
enum State {
    Request(Vec<u8>),
    Response {
        headers: Vec<u8>,
        body: Arc<[u8]>,
        written: usize,
    },
}
impl Connection {
    fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            deadline: Instant::now() + Duration::from_secs(2),
            state: State::Request(Vec::new()),
        }
    }
    fn advance(&mut self, assets: &BTreeMap<String, Arc<[u8]>>, changed: bool) -> bool {
        if Instant::now() >= self.deadline {
            return false;
        }
        match &mut self.state {
            State::Request(request) => {
                let mut chunk = [0; 2048];
                let remaining = MAX_HEADER_BYTES - request.len();
                if remaining == 0 {
                    return false;
                }
                match self.stream.read(&mut chunk[..remaining.min(2048)]) {
                    Ok(0) => return false,
                    Ok(count) => request.extend_from_slice(&chunk[..count]),
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                        ) =>
                    {
                        return true;
                    }
                    Err(_) => return false,
                }
                if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                    let text = String::from_utf8_lossy(&request[..end]);
                    let path = text.split_whitespace().nth(1).unwrap_or("");
                    self.state = response(
                        assets,
                        if path == "/" { "/index.html" } else { path },
                        changed,
                    );
                }
                true
            }
            State::Response {
                headers,
                body,
                written,
            } => {
                let bytes = if *written < headers.len() {
                    &headers[*written..]
                } else {
                    &body[*written - headers.len()..]
                };
                let bytes = &bytes[..bytes.len().min(WRITE_BYTES_PER_TURN)];
                match self.stream.write(bytes) {
                    Ok(0) => false,
                    Ok(count) => {
                        *written += count;
                        *written < headers.len() + body.len()
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                        ) =>
                    {
                        true
                    }
                    Err(_) => false,
                }
            }
        }
    }
}
fn response(assets: &BTreeMap<String, Arc<[u8]>>, path: &str, changed: bool) -> State {
    let original = assets.get(path);
    let mut body = original.cloned().unwrap_or_default();
    if changed {
        if path == "/worker.js" {
            let mut updated = body.to_vec();
            updated.extend_from_slice(b"\n// rolling deployment integrity regression\n");
            body = updated.into();
        }
        if path == "/style.css" {
            body = Arc::from(b"body { display: none !important; }".as_slice());
        }
    }
    let mime = if path.ends_with(".wasm") {
        "application/wasm"
    } else if path.ends_with(".js") {
        "text/javascript"
    } else if path.ends_with(".css") {
        "text/css"
    } else {
        "text/html"
    };
    let status = if original.is_some() {
        "200 OK"
    } else {
        "404 Not Found"
    };
    let headers = format!("HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",body.len()).into_bytes();
    State::Response {
        headers,
        body,
        written: 0,
    }
}

pub(super) fn run(browser: &mut Browser, source: &str) -> Result<()> {
    let mut assets = BTreeMap::new();
    for name in super::super::ASSETS.iter().copied().chain(["worker.js"]) {
        let path = format!("/{name}");
        let expression = format!(
            "(async()=>{{const response=await fetch({});if(!response.ok)throw Error('missing fixture asset');const bytes=new Uint8Array(await response.arrayBuffer());let raw='';for(let offset=0;offset<bytes.length;offset+=8192)raw+=String.fromCharCode(...bytes.subarray(offset,offset+8192));return btoa(raw);}})()",
            json!(path)
        );
        let encoded = browser.eval(source, &expression)?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded.as_str().ok_or("invalid fixture asset")?)
            .map_err(|e| e.to_string())?;
        assets.insert(path, bytes);
    }
    let fixture = Fixture::new(assets)?;
    let page = browser.page(&fixture.origin)?;
    browser.wait(
        &page,
        "navigator.serviceWorker.controller !== null && document.querySelector('#draft') !== null",
    )?;
    browser.eval(&page, "document.querySelector('#draft').value='Survives rejected upgrade';document.querySelector('#draft').dispatchEvent(new Event('input',{bubbles:true}));true")?;
    let before = browser.eval(&page, "(async()=>{const key=(await caches.keys()).find(k=>k.startsWith('fern-public-assets-v1-'));return await (await (await caches.open(key)).match('/style.css')).text()})()")?;
    fixture.changed.store(true, Ordering::Relaxed);
    // Attach before update so even a fast rejecting install is observable.
    browser.eval(&page, "(async()=>{const registration=await navigator.serviceWorker.getRegistration();window.fernRejectedUpdate=false;registration.addEventListener('updatefound',()=>{const worker=registration.installing;worker.addEventListener('statechange',()=>{if(worker.state==='redundant')window.fernRejectedUpdate=true})});await registration.update();return true})()")?;
    browser.wait(&page, "window.fernRejectedUpdate === true")?;
    let after = browser.eval(&page, "(async()=>{const key=(await caches.keys()).find(k=>k.startsWith('fern-public-assets-v1-'));return await (await (await caches.open(key)).match('/style.css')).text()})()")?;
    if before != after {
        return Err("failed worker upgrade modified the active asset cache".into());
    }
    browser.call(Some(&page), "Network.enable", json!({}))?;
    browser.call(
        Some(&page),
        "Network.emulateNetworkConditions",
        json!({"offline":true,"latency":0,"downloadThroughput":0,"uploadThroughput":0}),
    )?;
    browser.call(Some(&page), "ServiceWorker.enable", json!({}))?;
    browser.call(Some(&page), "ServiceWorker.stopAllWorkers", json!({}))?;
    browser.call(Some(&page), "Page.reload", json!({}))?;
    browser.wait(&page, "document.querySelector('#draft')?.value === 'Survives rejected upgrade' && getComputedStyle(document.body).display !== 'none' && document.querySelector('#status')?.textContent.includes('Offline') === true")?;
    println!(
        "Asset integrity acceptance passed: HTTP200 tampering rejected atomically; previous worker restarts offline"
    );
    Ok(())
}
