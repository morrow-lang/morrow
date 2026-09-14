//! Rust-owned local HTTP/browser harness. JavaScript below only drives measurements.
use serde_json::{Value, json};
use std::os::unix::{fs::DirBuilderExt, process::CommandExt};
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const MAX_RESULT: usize = 2 * 1024 * 1024;
const DRIVER: &str = r#"<!doctype html><meta charset="utf-8"><title>Morrow codec experiment</title><pre id="status">Starting</pre><script type="module">
const status = document.querySelector('#status');
try {
  const names = ['json', 'cbor', 'protobuf'];
  const modules = [];
  for (const name of names) {
    const m = await import(`/${name}/codec.js`);
    const exports = await m.default();
    const memoryBefore = exports.memory.buffer.byteLength;
    if (m.fixture_count() !== 46) throw Error('fixture count');
    modules.push({name, m, exports, memoryBefore, memoryPrepared: exports.memory.buffer.byteLength});
  }
  const rows = [];
  let sink = 0;
  function sample(action) {
    action(30);
    let n = 100;
    for (let attempts = 0; attempts < 5; attempts++) {
      const start = performance.now(); sink += action(n); const elapsed = performance.now() - start;
      if (elapsed >= 8 || n === 100000) break;
      n = Math.min(100000, Math.max(n + 1, Math.ceil(n * 10 / Math.max(elapsed, 0.01))));
    }
    const samples = [];
    for (let repeat = 0; repeat < 9; repeat++) {
      const start = performance.now(); sink += action(n);
      samples.push((performance.now() - start) * 1e6 / n);
    }
    const sorted = [...samples].sort((a,b) => a-b);
    return {iterations_per_sample:n, batch_mean_samples:samples, median:sorted[4], min:sorted[0], max:sorted[8]};
  }
  for (let index = 0; index < 46; index++) {
    for (let turn = 0; turn < 3; turn++) {
      const {name, m, exports} = modules[(index + turn) % 3];
      const bytes = m.encode_one(index);
      if (!m.check_one(index, bytes)) throw Error('semantic boundary mismatch');
      const nbytes = bytes.length;
      if (m.batch(index, 7, true) !== nbytes * 7 || m.batch(index, 7, false) !== 7) throw Error('batch checksum');
      const loop = action => n => {let sum = 0; for(let i=0;i<n;i++)sum += action(); return sum;};
      const actions = {
        in_wasm_encode_ns: n => m.batch(index,n,true),
        in_wasm_decode_validate_ns: n => m.batch(index,n,false),
        js_bytes_to_wasm_decode_validate_ns: loop(() => m.decode_one(index,bytes)),
        wasm_encode_to_js_bytes_ns: loop(() => m.encode_one(index).length),
        copy_js_bytes_to_wasm_ns: loop(() => m.copy_in(bytes)),
        copy_wasm_bytes_to_js_ns: loop(() => m.copy_out(index).length)
      };
      if (name === 'json') {
        const text = new TextDecoder('utf-8',{fatal:true}).decode(bytes);
        if (m.decode_text(index,text) !== 1 || m.encode_text(index) !== text) throw Error('text boundary mismatch');
        actions.js_text_to_wasm_decode_validate_ns = loop(() => m.decode_text(index,text));
        actions.wasm_encode_to_js_text_ns = loop(() => m.encode_text(index).length);
        actions.copy_js_text_to_wasm_ns = loop(() => m.copy_text_in(text));
        actions.copy_wasm_text_to_js_ns = loop(() => m.copy_text_out(index).length);
      }
      const phases = Object.keys(actions);
      const rotate = index % phases.length;
      phases.push(...phases.splice(0,rotate));
      const measurements = {};
      const memoryBefore = exports.memory.buffer.byteLength;
      for (const phase of phases) measurements[phase] = sample(actions[phase]);
      rows.push({fixture:m.fixture_name(index),codec:name,bytes:nbytes,memory_before:memoryBefore,memory_after:exports.memory.buffer.byteLength,measurements});
      status.textContent = `${rows.length}/138`;
      await new Promise(resolve => setTimeout(resolve,0));
    }
  }
  const result = {format_version:1,kind:'real_browser_wasm_codec_microbenchmark',user_agent:navigator.userAgent,hardware_concurrency:navigator.hardwareConcurrency,cross_origin_isolated:crossOriginIsolated,time_origin:performance.timeOrigin,samples:9,target_batch_ms:8,max_iterations:100000,fixture_count:46,codec_order:'rotated by fixture index',phase_order:'rotated by fixture index',memory:modules.map(({name,exports,memoryBefore,memoryPrepared})=>({codec:name,before_fixture_bytes:memoryBefore,prepared_bytes:memoryPrepared,final_bytes:exports.memory.buffer.byteLength})),checksum:sink,rows};
  const response = await fetch('/result',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(result)});
  if (!response.ok) throw Error('result upload');
  status.textContent = 'Complete';
} catch (error) {
  status.textContent = String(error.stack || error);
  await fetch('/result',{method:'POST',body:JSON.stringify({error:String(error.stack || error)})});
}
</script>"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    match args.first().and_then(|arg| arg.to_str()) {
        Some("prepare") if args.len() == 2 => prepare(Path::new(&args[1])),
        Some("run") if args.len() == 4 => run(
            Path::new(&args[1]),
            Path::new(&args[2]),
            Path::new(&args[3]),
        ),
        _ => Err(
            "usage: browser prepare NEW_ASSET_DIR | browser run ASSET_DIR BROWSER NEW_RESULT_FILE"
                .into(),
        ),
    }
}

fn prepare(directory: &Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::DirBuilder::new().mode(0o700).create(directory)?;
    fs::write(directory.join("index.html"), DRIVER)?;
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let target = std::env::var_os("CARGO_TARGET_DIR").ok_or("set absolute CARGO_TARGET_DIR")?;
    if !Path::new(&target).is_absolute() {
        return Err("CARGO_TARGET_DIR must be absolute".into());
    }
    let bindgen = std::env::var_os("WASM_BINDGEN").unwrap_or_else(|| "wasm-bindgen".into());
    let mut artifacts = Vec::new();
    for codec in ["json", "cbor", "protobuf"] {
        checked(
            Command::new("cargo")
                .args(["build", "--manifest-path"])
                .arg(&manifest)
                .args([
                    "--locked",
                    "--release",
                    "--target",
                    "wasm32-unknown-unknown",
                    "--lib",
                    "--features",
                    &format!("browser-{codec}"),
                ]),
        )?;
        checked(
            Command::new(&bindgen)
                .arg(
                    Path::new(&target)
                        .join("wasm32-unknown-unknown/release/morrow_network_codecs.wasm"),
                )
                .args(["--target", "web", "--out-name", "codec", "--out-dir"])
                .arg(directory.join(codec)),
        )?;
        for name in ["codec.js", "codec_bg.wasm"] {
            let file = directory.join(codec).join(name);
            let gzip = Command::new("gzip")
                .args(["-n", "-c"])
                .arg(&file)
                .output()?;
            if !gzip.status.success() {
                return Err("gzip failed".into());
            }
            artifacts.push(json!({"codec":codec,"file":name,"bytes":fs::metadata(&file)?.len(),"gzip_bytes":gzip.stdout.len(),"sha256":sha256(&file)?}));
        }
    }
    let mut sources = Vec::new();
    for relative in [
        "Cargo.toml",
        "Cargo.lock",
        "src/lib.rs",
        "src/adapter.rs",
        "src/schema.rs",
        "src/validate.rs",
        "src/corpus.rs",
        "src/browser.rs",
        "src/bin/browser.rs",
    ] {
        sources.push(json!({"path":relative,"sha256":sha256(&Path::new(env!("CARGO_MANIFEST_DIR")).join(relative))?}));
    }
    let metadata = json!({"rustc":output(Command::new("rustc").arg("--version"))?,"wasm_bindgen":output(Command::new(bindgen).arg("--version"))?,"profile":"release, thin LTO, codegen-units=1, stripped; separately compile-selected codecs; no wasm-opt","artifacts":artifacts,"sources":sources,"unix_seconds":SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()});
    fs::write(
        directory.join("metadata.json"),
        serde_json::to_vec_pretty(&metadata)?,
    )?;
    println!("{}", directory.display());
    Ok(())
}

fn checked(command: &mut Command) -> Result<(), Box<dyn std::error::Error>> {
    if !command.status()?.success() {
        return Err(format!("command failed: {command:?}").into());
    }
    Ok(())
}
fn output(command: &mut Command) -> Result<String, Box<dyn std::error::Error>> {
    let output = command.output()?;
    if !output.status.success() {
        return Err(format!("command failed: {command:?}").into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().into())
}
fn sha256(file: &Path) -> Result<String, Box<dyn std::error::Error>> {
    Ok(
        output(Command::new("shasum").args(["-a", "256"]).arg(file))?
            .split_whitespace()
            .next()
            .ok_or("missing hash")?
            .into(),
    )
}

struct OwnedBrowser(Child);
impl Drop for OwnedBrowser {
    fn drop(&mut self) {
        // The child has not been reaped and retains its PID while its dedicated
        // process group is killed. Reap only after signalling the owned group.
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
        let _ = self.0.wait();
    }
}

fn run(
    directory: &Path,
    browser: &Path,
    result_file: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let profile: PathBuf = directory.join(format!(
        "run-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    fs::DirBuilder::new().mode(0o700).create(&profile)?;
    let log = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(profile.join("browser.log"))?;
    let mut command = Command::new(browser);
    command
        .args([
            "--headless=new",
            "--disable-background-networking",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-component-update",
        ])
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg(format!("http://{}/", listener.local_addr()?))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log)
        .process_group(0);
    let _child = OwnedBrowser(command.spawn()?);
    let deadline = Instant::now() + Duration::from_secs(300);
    while Instant::now() < deadline {
        match listener.accept() {
            Ok((stream, _)) => {
                if let Some(mut result) = serve(stream, directory)? {
                    if result.get("error").is_some() {
                        return Err(format!("browser: {result}").into());
                    }
                    if result["rows"].as_array().map(Vec::len) != Some(138) {
                        return Err("incomplete browser results".into());
                    }
                    result["build"] =
                        serde_json::from_slice(&fs::read(directory.join("metadata.json"))?)?;
                    result["browser_executable_version"] =
                        json!(output(Command::new(browser).arg("--version"))?);
                    let mut file = fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(result_file)?;
                    file.write_all(&serde_json::to_vec_pretty(&result)?)?;
                    println!("{}", result_file.display());
                    return Ok(());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(error) => return Err(error.into()),
        }
    }
    Err(format!(
        "browser exceeded 300 seconds; log {}",
        profile.join("browser.log").display()
    )
    .into())
}

fn serve(
    mut stream: TcpStream,
    directory: &Path,
) -> Result<Option<Value>, Box<dyn std::error::Error>> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut request = Vec::new();
    let header_end = loop {
        if let Some(index) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            break index + 4;
        }
        if request.len() >= 16_384 {
            return Err("request header too large".into());
        }
        let mut chunk = [0; 1024];
        let count = match stream.read(&mut chunk) {
            Ok(count) => count,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return Ok(None);
            }
            Err(error) => return Err(error.into()),
        };
        if count == 0 {
            return Ok(None);
        }
        request.extend_from_slice(&chunk[..count]);
    };
    let header = std::str::from_utf8(&request[..header_end])?;
    let first = header.lines().next().ok_or("missing request")?;
    if first == "POST /result HTTP/1.1" {
        let length = header
            .lines()
            .find_map(|line| {
                line.split_once(':')
                    .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                    .map(|(_, value)| value.trim().parse::<usize>())
            })
            .ok_or("missing content length")??;
        if length > MAX_RESULT {
            return Err("result too large".into());
        }
        while request.len() - header_end < length {
            let remaining = length - (request.len() - header_end);
            let mut chunk = [0; 8192];
            let count = stream.read(&mut chunk[..remaining.min(8192)])?;
            if count == 0 {
                return Err("truncated result".into());
            }
            request.extend_from_slice(&chunk[..count]);
        }
        let value = serde_json::from_slice(&request[header_end..header_end + length])?;
        respond(&mut stream, "200 OK", "text/plain", b"ok")?;
        return Ok(Some(value));
    }
    let relative = if first == "GET / HTTP/1.1" {
        Some("index.html".to_string())
    } else {
        first
            .strip_prefix("GET /")
            .and_then(|line| line.strip_suffix(" HTTP/1.1"))
            .filter(|path| {
                [
                    "json/codec.js",
                    "json/codec_bg.wasm",
                    "cbor/codec.js",
                    "cbor/codec_bg.wasm",
                    "protobuf/codec.js",
                    "protobuf/codec_bg.wasm",
                ]
                .contains(path)
            })
            .map(str::to_owned)
    };
    if let Some(relative) = relative {
        let content_type = if relative.ends_with(".wasm") {
            "application/wasm"
        } else if relative.ends_with(".js") {
            "text/javascript"
        } else {
            "text/html"
        };
        respond(
            &mut stream,
            "200 OK",
            content_type,
            &fs::read(directory.join(relative))?,
        )?;
    } else {
        respond(&mut stream, "404 Not Found", "text/plain", b"not found")?;
    }
    Ok(None)
}
fn respond(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    bytes: &[u8],
) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nCross-Origin-Opener-Policy: same-origin\r\nCross-Origin-Embedder-Policy: require-corp\r\n\r\n",
        bytes.len()
    )?;
    stream.write_all(bytes)
}
