//! A rolling deployment must never install HTTP-successful bytes from another revision.
use super::{Browser, Result};
use base64::Engine as _;
use serde_json::json;
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
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
        let thread = thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let Ok((mut stream, _)) = listener.accept() else {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                let mut request = Vec::new();
                let mut bytes = [0; 2048];
                while request.len() < 16 * 1024 && !request.windows(4).any(|v| v == b"\r\n\r\n") {
                    match stream.read(&mut bytes) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => request.extend_from_slice(&bytes[..n]),
                    }
                }
                let text = String::from_utf8_lossy(&request);
                let path = text.split_whitespace().nth(1).unwrap_or("");
                let path = if path == "/" { "/index.html" } else { path };
                let original = assets.get(path);
                let mut body = original.cloned().unwrap_or_default();
                if deployment.load(Ordering::Relaxed) {
                    if path == "/worker.js" {
                        body.extend_from_slice(b"\n// rolling deployment integrity regression\n");
                    }
                    if path == "/style.css" {
                        body = b"body { display: none !important; }".to_vec();
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
                let headers = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(headers.as_bytes());
                let _ = stream.write_all(&body);
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
