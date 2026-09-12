//! Real Chromium acceptance uses owned processes/profiles and the DevTools protocol.
mod integrity;
use base64::Engine as _;
use serde_json::{Value, json};
use std::{
    env, fs,
    net::TcpStream,
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use tungstenite::{Message, WebSocket, stream::MaybeTlsStream};

type Result<T> = std::result::Result<T, String>;
const WAIT: Duration = Duration::from_secs(15);

struct Process(Child);
impl Process {
    fn spawn(command: &mut Command) -> Result<Self> {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
        command.spawn().map(Self).map_err(|error| error.to_string())
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        // SAFETY: this child starts its own process group; retain its wait identity
        // until all group signals finish so the group id cannot be reused.
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGTERM);
        }
        thread::sleep(Duration::from_millis(25));
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
        let _ = self.0.wait();
    }
}

struct Browser {
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
    next: u64,
}
impl Browser {
    fn connect(url: &str) -> Result<Self> {
        let (mut socket, _) = tungstenite::connect(url).map_err(|error| error.to_string())?;
        if let MaybeTlsStream::Plain(stream) = socket.get_mut() {
            stream
                .set_read_timeout(Some(WAIT))
                .map_err(|error| error.to_string())?;
            stream
                .set_write_timeout(Some(Duration::from_secs(2)))
                .map_err(|error| error.to_string())?;
        }
        Ok(Self { socket, next: 0 })
    }
    fn call(&mut self, session: Option<&str>, method: &str, params: Value) -> Result<Value> {
        self.next += 1;
        let mut request = json!({"id":self.next,"method":method,"params":params});
        if let Some(session) = session {
            request["sessionId"] = session.into();
        }
        self.socket
            .send(Message::Text(request.to_string().into()))
            .map_err(|error| error.to_string())?;
        let start = Instant::now();
        for _ in 0..10_000 {
            if start.elapsed() > WAIT {
                break;
            }
            let message = self
                .socket
                .read()
                .map_err(|error| format!("DevTools {method}: {error}"))?;
            if let Message::Text(text) = message {
                if text.len() > 4 * 1024 * 1024 {
                    return Err("DevTools response exceeds 4 MiB".into());
                }
                let response: Value =
                    serde_json::from_str(&text).map_err(|error| error.to_string())?;
                if response["id"].as_u64() == Some(self.next) {
                    if !response["error"].is_null() {
                        return Err(format!("DevTools {method}: {}", response["error"]));
                    }
                    return Ok(response["result"].clone());
                }
            }
        }
        Err(format!("DevTools {method} exceeded response budget"))
    }
    fn page(&mut self, origin: &str) -> Result<String> {
        let target = self.call(None, "Target.createTarget", json!({"url":origin}))?;
        let attached = self.call(
            None,
            "Target.attachToTarget",
            json!({"targetId":target["targetId"],"flatten":true}),
        )?;
        let session = attached["sessionId"]
            .as_str()
            .ok_or("missing browser session")?
            .to_owned();
        self.call(Some(&session), "Runtime.enable", json!({}))?;
        self.call(Some(&session), "Page.enable", json!({}))?;
        Ok(session)
    }
    fn eval(&mut self, session: &str, expression: &str) -> Result<Value> {
        let result = self.call(
            Some(session),
            "Runtime.evaluate",
            json!({"expression":expression,"awaitPromise":true,"returnByValue":true}),
        )?;
        if !result["exceptionDetails"].is_null() {
            return Err(format!(
                "browser evaluation: {}",
                result["exceptionDetails"]
            ));
        }
        Ok(result["result"]["value"].clone())
    }
    fn wait(&mut self, session: &str, expression: &str) -> Result<()> {
        let start = Instant::now();
        while start.elapsed() < WAIT {
            if self.eval(session, expression)? == true {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(25));
        }
        let state = self.eval(session, "document.body?.innerText")?;
        Err(format!(
            "browser condition timed out: {expression}\nPage: {state}"
        ))
    }
}

fn readiness(path: &Path, predicate: impl Fn(&str) -> Option<String>) -> Result<String> {
    let start = Instant::now();
    while start.elapsed() < WAIT {
        if let Ok(text) = fs::read_to_string(path)
            && let Some(value) = predicate(&text)
        {
            return Ok(value);
        }
        thread::sleep(Duration::from_millis(25));
    }
    Err(format!(
        "readiness timeout: {}\n{}",
        path.display(),
        fs::read_to_string(path).unwrap_or_default()
    ))
}

/// Exercise actual generated browser artifacts and transport, including offline reload.
pub fn run(server: &Path) -> Result<()> {
    run_mode(server, false)
}

/// Isolated real-browser upgrade oracle, also used to prove the unfixed worker fails.
pub fn run_integrity(server: &Path) -> Result<()> {
    run_mode(server, true)
}

fn run_mode(server: &Path, integrity_only: bool) -> Result<()> {
    let browser_path = env::var_os("FERN_BROWSER")
        .ok_or("set FERN_BROWSER to a Chromium/Edge executable for real-browser acceptance")?;
    let directory = crate::Temporary::new(&env::temp_dir())?;
    let server_log = directory.0.join("server.log");
    let mut server_process = None;
    let origin = if let Ok(origin) = env::var("FERN_WEB_CHECK_ORIGIN") {
        let address: std::net::SocketAddr = origin
            .strip_prefix("http://")
            .ok_or("external acceptance origin must be a loopback http socket address")?
            .parse()
            .map_err(|_| "invalid acceptance origin")?;
        if !address.ip().is_loopback() {
            return Err("acceptance origin must be loopback".into());
        }
        origin
    } else {
        let server = server.canonicalize().map_err(|error| error.to_string())?;
        server_process = Some(Process::spawn(
            Command::new(server)
                .env("FERN_WEB_BIND", "127.0.0.1:0")
                .env_remove("FERN_WEB_ORIGIN")
                .env("FERN_WEB_ACCESS_KEY", "fern-browser-test-key")
                .stdout(Stdio::null())
                .stderr(fs::File::create(&server_log).map_err(|error| error.to_string())?),
        )?);
        readiness(&server_log, |text| {
            text.lines().find_map(|line| {
                line.split_once("browser origin ")
                    .map(|(_, origin)| origin.to_owned())
            })
        })?
    };
    let profile = directory.0.join("profile");
    let browser_log = directory.0.join("browser.log");
    let _browser = Process::spawn(
        Command::new(browser_path)
            .args([
                "--headless=new",
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-background-networking",
                "--remote-debugging-port=0",
                "--window-size=1280,960",
            ])
            .arg(format!("--user-data-dir={}", profile.display()))
            .arg("about:blank")
            .stdout(Stdio::null())
            .stderr(fs::File::create(&browser_log).map_err(|error| error.to_string())?),
    )?;
    let endpoint = readiness(&profile.join("DevToolsActivePort"), |text| {
        let mut lines = text.lines();
        let port = lines.next()?.parse::<u16>().ok()?;
        let path = lines.next()?.strip_prefix("/devtools/browser/")?;
        Some(format!("ws://127.0.0.1:{port}/devtools/browser/{path}"))
    })?;
    let mut browser = Browser::connect(&endpoint)?;
    let first = browser.page(&origin)?;
    if integrity_only {
        browser.wait(&first, "document.querySelector('#draft') !== null")?;
        return integrity::run(&mut browser, &first);
    }
    browser.wait(
        &first,
        "document.querySelector('#status')?.textContent.includes('access key') === true",
    )?;
    browser.eval(&first, "document.querySelector('#access-key').value='fern-browser-test-key'; document.querySelector('#login-form').requestSubmit(); true")?;
    browser.wait(
        &first,
        "document.querySelector('#connection')?.getAttribute('data-online') === 'true'",
    )?;
    let second = browser.page(&origin)?;
    browser.wait(
        &second,
        "document.querySelector('#connection')?.getAttribute('data-online') === 'true'",
    )?;
    browser.eval(&first, "document.querySelector('#draft').value='Grow a lasting language'; document.querySelector('#draft').dispatchEvent(new Event('input',{bubbles:true})); document.querySelector('#add-form').requestSubmit(); true")?;
    for page in [&first, &second] {
        browser.wait(page, "document.querySelector('#tasks')?.textContent.includes('Grow a lasting language') === true")?;
        browser.wait(
            page,
            "getComputedStyle(document.querySelector('#empty')).display === 'none'",
        )?;
    }
    browser.eval(
        &second,
        "document.querySelector('#tasks input[type=checkbox]').click(); true",
    )?;
    for page in [&first, &second] {
        browser.wait(
            page,
            "document.querySelector('#tasks input[type=checkbox]')?.checked === true",
        )?;
    }
    browser.wait(
        &second,
        "document.querySelector('#tasks input[type=checkbox]')?.disabled === false",
    )?;
    browser.eval(&first, "document.querySelector('#filter-1').click(); true")?;
    browser.wait(
        &first,
        "document.querySelector('#tasks li')?.hidden === true",
    )?;
    browser.eval(&first, "document.querySelector('#filter-0').click(); true")?;
    browser.eval(&first, "document.querySelector('#draft').focus(); document.querySelector('#draft').value='My offline draft'; document.querySelector('#draft').dispatchEvent(new Event('input',{bubbles:true})); document.querySelector('#draft').setSelectionRange(3,7); true")?;
    browser.eval(
        &second,
        "document.querySelector('#tasks input[type=checkbox]').click(); true",
    )?;
    browser.wait(
        &first,
        "document.querySelector('#tasks input[type=checkbox]')?.checked === false",
    )?;
    if browser.eval(&first, "document.activeElement?.id === 'draft' && document.querySelector('#draft').selectionStart === 3 && document.querySelector('#draft').selectionEnd === 7")? != true {
        return Err("remote update moved local focus or selection".into());
    }
    browser.wait(&first, "navigator.serviceWorker.controller !== null")?;
    browser.call(Some(&first), "Network.enable", json!({}))?;
    browser.call(
        Some(&first),
        "Network.emulateNetworkConditions",
        json!({"offline":true,"latency":0,"downloadThroughput":0,"uploadThroughput":0}),
    )?;
    browser.call(Some(&first), "ServiceWorker.enable", json!({}))?;
    browser.call(Some(&first), "ServiceWorker.stopAllWorkers", json!({}))?;
    browser.call(Some(&first), "Page.reload", json!({}))?;
    browser.wait(&first, "document.querySelector('#draft')?.value === 'My offline draft' && document.querySelector('#tasks')?.textContent.includes('Grow a lasting language') === true && document.querySelector('#status')?.textContent.includes('Offline') === true")?;
    if browser.eval(&first, "[...document.querySelectorAll('#add, #tasks input, #tasks button')].every(control => control.disabled)")? != true {
        return Err("offline mutation submission was enabled".into());
    }
    browser.eval(&first, "document.querySelector('#filter-2').click(); true")?;
    browser.wait(
        &first,
        "document.querySelector('#tasks li')?.hidden === true",
    )?;
    browser.eval(&first, "document.querySelector('#filter-0').click(); true")?;
    browser.call(
        Some(&first),
        "Network.emulateNetworkConditions",
        json!({"offline":false,"latency":0,"downloadThroughput":-1,"uploadThroughput":-1}),
    )?;
    browser.eval(&first, "document.querySelector('#retry').click(); true")?;
    browser.wait(
        &first,
        "document.querySelector('#connection')?.getAttribute('data-online') === 'true'",
    )?;
    if browser.eval(&first, "document.querySelector('#draft').value")? != "My offline draft" {
        return Err("reconnect lost the offline draft".into());
    }
    let screenshot = browser.call(
        Some(&first),
        "Page.captureScreenshot",
        json!({"format":"png"}),
    )?;
    if let Some(path) = env::var_os("FERN_WEB_SCREENSHOT") {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(screenshot["data"].as_str().ok_or("missing screenshot")?)
            .map_err(|error| error.to_string())?;
        fs::write(path, bytes).map_err(|error| error.to_string())?;
    }
    browser.call(
        Some(&first),
        "Emulation.setDeviceMetricsOverride",
        json!({"width":320,"height":740,"deviceScaleFactor":1,"mobile":true}),
    )?;
    if browser.eval(&first, "document.documentElement.scrollWidth <= window.innerWidth && document.querySelector('#draft').getBoundingClientRect().width >= 100")? != true {
        return Err("mobile layout overflows or leaves no usable draft field".into());
    }
    if let Some(path) = env::var_os("FERN_WEB_MOBILE_SCREENSHOT") {
        let screenshot = browser.call(
            Some(&first),
            "Page.captureScreenshot",
            json!({"format":"png","captureBeyondViewport":true}),
        )?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(
                screenshot["data"]
                    .as_str()
                    .ok_or("missing mobile screenshot")?,
            )
            .map_err(|error| error.to_string())?;
        fs::write(path, bytes).map_err(|error| error.to_string())?;
    }
    browser.eval(&first, "document.querySelector('#logout').click(); true")?;
    browser.wait(
        &second,
        "document.querySelector('#connection')?.getAttribute('data-online') === 'false'",
    )?;
    integrity::run(&mut browser, &first)?;
    println!(
        "Real-browser acceptance passed: two clients, compiled Fern policy, keyed DOM/focus, offline worker restart/reload/draft/filter, mobile layout, reconnect and session revocation"
    );
    drop(server_process);
    Ok(())
}
