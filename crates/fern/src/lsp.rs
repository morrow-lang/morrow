//! Bounded JSON-RPC transport and UTF-16 editor diagnostics.
//! Lifecycle and sync follow https://microsoft.github.io/language-server-protocol/.
use crate::{Span, Type, ast, check, modules, parse, runtime};
#[path = "lsp/actions.rs"]
mod actions;
#[path = "lsp/capabilities.rs"]
mod capabilities;
#[path = "lsp/formatting.rs"]
mod formatting;
#[path = "lsp/hover.rs"]
mod hover;
#[path = "lsp/index.rs"]
mod index;
#[path = "lsp/navigation.rs"]
mod navigation;
mod recovery;
#[path = "lsp/refactor.rs"]
mod refactor;
#[cfg(test)]
#[path = "lsp/refactor_tests.rs"]
mod refactor_tests;
use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};

const MAX_FRAME: usize = 8 * 1024 * 1024;
const MAX_HEADER: usize = 8192;
const MAX_SOURCE: usize = 1024 * 1024;
const MAX_DOCUMENTS: usize = 128;
const MAX_TOTAL_SOURCE: usize = 16 * MAX_SOURCE;
const MAX_JSON_DEPTH: usize = 64;
const MAX_JSON_NODES: usize = 200_000;
type Result<T> = std::result::Result<T, String>;

/// Serve JSON-RPC with bounded disk imports overlaid by unsaved editor buffers.
/// Frames, JSON depth/nodes, documents and change batches have explicit memory bounds.
/// Returns success after shutdown plus exit/EOF, or an error for transport/early exit.
pub fn serve(mut input: impl BufRead, mut output: impl Write) -> Result<()> {
    let mut server = Server {
        state: State::New,
        client_edits: capabilities::ClientEdits::default(),
        documents: BTreeMap::new(),
        published: BTreeMap::new(),
    };
    // A language-server session intentionally lasts until its transport closes or exit arrives.
    loop {
        let Some(frame) = read_frame(&mut input)? else {
            return if server.state == State::Shutdown {
                Ok(())
            } else {
                Err("LSP input closed before shutdown".into())
            };
        };
        let message = match JsonParser::parse(&frame) {
            Ok(message) => message,
            Err(error) => {
                send_error(&mut output, Json::Null, -32700, &error)?;
                continue;
            }
        };
        if server.message(message, &mut output)? {
            return Ok(());
        }
    }
}

/// Read strictly bounded ASCII headers and an exact UTF-8 JSON payload.
fn read_frame(input: &mut impl BufRead) -> Result<Option<String>> {
    let mut length = None;
    let mut consumed = 0;
    for _ in 0..32 {
        let mut line = Vec::new();
        let count = (&mut *input)
            .take((MAX_HEADER - consumed + 1) as u64)
            .read_until(b'\n', &mut line)
            .map_err(|error| error.to_string())?;
        if count == 0 {
            return if consumed == 0 {
                Ok(None)
            } else {
                Err("truncated LSP header".into())
            };
        }
        consumed += count;
        if consumed > MAX_HEADER {
            return Err("LSP header exceeds 8192 bytes".into());
        }
        if !line.ends_with(b"\r\n") {
            return Err("LSP headers require CRLF line endings".into());
        }
        if line == b"\r\n" {
            let length = length.ok_or("missing Content-Length header")?;
            let mut body = vec![0; length];
            input
                .read_exact(&mut body)
                .map_err(|error| format!("truncated LSP payload: {error}"))?;
            return String::from_utf8(body)
                .map(Some)
                .map_err(|error| format!("LSP payload is not UTF-8: {error}"));
        }
        if !line.is_ascii() {
            return Err("LSP headers must be ASCII".into());
        }
        let line = std::str::from_utf8(&line).map_err(|error| error.to_string())?;
        read_header(line, &mut length)?;
    }
    Err("too many LSP headers".into())
}

/// Validate one header field without allocating unbounded framing buffers.
fn read_header(line: &str, length: &mut Option<usize>) -> Result<()> {
    let (name, value) = line
        .trim_end()
        .split_once(':')
        .ok_or("malformed LSP header")?;
    if name.eq_ignore_ascii_case("Content-Length") {
        if length.is_some() {
            return Err("duplicate Content-Length header".into());
        }
        let value = value.trim();
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("invalid Content-Length".into());
        }
        let value = value
            .parse::<usize>()
            .map_err(|_| "invalid Content-Length")?;
        if value == 0 || value > MAX_FRAME {
            return Err("Content-Length exceeds the 8 MiB frame limit".into());
        }
        *length = Some(value);
    } else if name.eq_ignore_ascii_case("Content-Type") {
        let lower = value.to_ascii_lowercase();
        if let Some((_, charset)) = lower.split_once("charset=")
            && !["utf-8", "utf8"].contains(&charset.trim())
        {
            return Err("LSP supports only UTF-8 payloads".into());
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<Json>),
    Object(BTreeMap<String, Json>),
}

impl Json {
    /// Borrow one named object property without assuming the incoming JSON shape.
    fn get(&self, name: &str) -> Option<&Json> {
        if let Self::Object(fields) = self {
            fields.get(name)
        } else {
            None
        }
    }
    /// Read a JSON string, rejecting all other types.
    fn string(&self) -> Result<&str> {
        if let Self::String(value) = self {
            Ok(value)
        } else {
            Err("expected JSON string".into())
        }
    }
    /// Read an exact signed integer without floating-point rounding.
    fn integer(&self) -> Result<i64> {
        if let Self::Number(value) = self {
            value.parse().map_err(|_| "expected JSON integer".into())
        } else {
            Err("expected JSON integer".into())
        }
    }
    /// Read an array while retaining borrowed document/change payloads.
    fn array(&self) -> Result<&[Json]> {
        if let Self::Array(values) = self {
            Ok(values)
        } else {
            Err("expected JSON array".into())
        }
    }
}

struct JsonParser<'a> {
    source: &'a str,
    at: usize,
    nodes: usize,
}

impl<'a> JsonParser<'a> {
    /// Parse exactly one bounded JSON value, rejecting trailing data and duplicate keys.
    fn parse(source: &'a str) -> Result<Json> {
        let mut parser = Self {
            source,
            at: 0,
            nodes: 0,
        };
        let value = parser.value(0)?;
        parser.space();
        if parser.at != source.len() {
            return Err("trailing data after JSON value".into());
        }
        Ok(value)
    }
    /// Borrow the next byte; all parsing cursors stay on checked UTF-8 boundaries.
    fn peek(&self) -> Option<u8> {
        self.source.as_bytes().get(self.at).copied()
    }
    /// Skip only the four whitespace bytes permitted by JSON.
    fn space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.at += 1;
        }
    }
    /// Consume a required punctuation byte without advancing on failure.
    fn expect(&mut self, byte: u8) -> Result<()> {
        if self.peek() != Some(byte) {
            return Err(format!("expected JSON punctuation {:?}", char::from(byte)));
        }
        self.at += 1;
        Ok(())
    }
    /// Parse recursively under explicit depth and aggregate node limits.
    fn value(&mut self, depth: usize) -> Result<Json> {
        if depth > MAX_JSON_DEPTH || self.nodes >= MAX_JSON_NODES {
            return Err("JSON depth or node limit exceeded".into());
        }
        self.nodes += 1;
        self.space();
        match self.peek() {
            Some(b'"') => self.text().map(Json::String),
            Some(b'[') => self.array(depth + 1),
            Some(b'{') => self.object(depth + 1),
            Some(b'-' | b'0'..=b'9') => self.number().map(Json::Number),
            Some(b'n') => {
                self.keyword("null")?;
                Ok(Json::Null)
            }
            Some(b't') => {
                self.keyword("true")?;
                Ok(Json::Bool(true))
            }
            Some(b'f') => {
                self.keyword("false")?;
                Ok(Json::Bool(false))
            }
            _ => Err("expected JSON value".into()),
        }
    }
    /// Consume a literal keyword, leaving delimiter validation to the containing value.
    fn keyword(&mut self, keyword: &str) -> Result<()> {
        if !self.source[self.at..].starts_with(keyword) {
            return Err("invalid JSON keyword".into());
        }
        self.at += keyword.len();
        Ok(())
    }
    /// Read an array with mandatory separators and no trailing commas.
    fn array(&mut self, depth: usize) -> Result<Json> {
        self.expect(b'[')?;
        self.space();
        let mut values = Vec::new();
        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(Json::Array(values));
        }
        for _ in 0..MAX_JSON_NODES {
            values.push(self.value(depth)?);
            self.space();
            if self.peek() == Some(b']') {
                self.at += 1;
                return Ok(Json::Array(values));
            }
            self.expect(b',')?;
        }
        Err("JSON array limit exceeded".into())
    }
    /// Read an object with unique keys so protocol metadata cannot be ambiguous.
    fn object(&mut self, depth: usize) -> Result<Json> {
        self.expect(b'{')?;
        self.space();
        let mut values = BTreeMap::new();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(Json::Object(values));
        }
        for _ in 0..MAX_JSON_NODES {
            self.space();
            let name = self.text()?;
            self.space();
            self.expect(b':')?;
            let value = self.value(depth)?;
            if values.insert(name, value).is_some() {
                return Err("duplicate JSON object key".into());
            }
            self.space();
            if self.peek() == Some(b'}') {
                self.at += 1;
                return Ok(Json::Object(values));
            }
            self.expect(b',')?;
        }
        Err("JSON object limit exceeded".into())
    }
    /// Parse JSON number grammar while preserving precision in request identifiers.
    fn number(&mut self) -> Result<String> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        match self.peek() {
            Some(b'0') => self.at += 1,
            Some(b'1'..=b'9') => self.digits()?,
            _ => return Err("invalid JSON number".into()),
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            self.digits()?;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            self.digits()?;
        }
        Ok(self.source[start..self.at].into())
    }
    /// Require at least one ASCII digit, bounded by the frame's byte length.
    fn digits(&mut self) -> Result<()> {
        let start = self.at;
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.at += 1;
        }
        if start == self.at {
            Err("expected digit in JSON number".into())
        } else {
            Ok(())
        }
    }
    /// Decode UTF-8 strings and JSON escapes without accepting raw control characters.
    fn text(&mut self) -> Result<String> {
        self.expect(b'"')?;
        let mut output = String::new();
        for _ in 0..self.source.len() {
            match self.peek() {
                Some(b'"') => {
                    self.at += 1;
                    return Ok(output);
                }
                Some(b'\\') => {
                    self.at += 1;
                    output.push(self.escape()?);
                }
                Some(0..=31) => return Err("unescaped control character in JSON string".into()),
                Some(_) => {
                    let character = self.source[self.at..]
                        .chars()
                        .next()
                        .ok_or("unterminated JSON string")?;
                    output.push(character);
                    self.at += character.len_utf8();
                }
                None => return Err("unterminated JSON string".into()),
            }
        }
        Err("JSON string limit exceeded".into())
    }
    /// Decode escapes including paired UTF-16 surrogates into Unicode scalar values.
    fn escape(&mut self) -> Result<char> {
        let byte = self.peek().ok_or("unterminated JSON escape")?;
        self.at += 1;
        match byte {
            b'"' => Ok('"'),
            b'\\' => Ok('\\'),
            b'/' => Ok('/'),
            b'b' => Ok('\u{8}'),
            b'f' => Ok('\u{c}'),
            b'n' => Ok('\n'),
            b'r' => Ok('\r'),
            b't' => Ok('\t'),
            b'u' => {
                let first = self.hex_quad()?;
                let value = if (0xd800..=0xdbff).contains(&first) {
                    self.expect(b'\\')?;
                    self.expect(b'u')?;
                    let second = self.hex_quad()?;
                    if !(0xdc00..=0xdfff).contains(&second) {
                        return Err("invalid JSON surrogate pair".into());
                    }
                    0x10000 + ((first - 0xd800) << 10) + second - 0xdc00
                } else {
                    first
                };
                char::from_u32(value).ok_or_else(|| "invalid JSON Unicode scalar".into())
            }
            _ => Err("invalid JSON escape".into()),
        }
    }
    /// Decode exactly four hex digits without indexing past the incoming frame.
    fn hex_quad(&mut self) -> Result<u32> {
        let mut value = 0;
        for _ in 0..4 {
            let byte = self.peek().ok_or("truncated Unicode escape")?;
            let digit = char::from(byte)
                .to_digit(16)
                .ok_or("invalid Unicode escape")?;
            value = (value << 4) | digit;
            self.at += 1;
        }
        Ok(value)
    }
}

/// Build an owned JSON object from fixed protocol field names.
fn object<const N: usize>(fields: [(&str, Json); N]) -> Json {
    Json::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect(),
    )
}
/// Convert an integer to an exact JSON number.
fn number(value: impl ToString) -> Json {
    Json::Number(value.to_string())
}
/// Convert text to a JSON string, leaving escaping to the transport writer.
fn string(value: impl Into<String>) -> Json {
    Json::String(value.into())
}
/// Require a named field without assuming the shape of client-supplied parameters.
fn field<'a>(value: &'a Json, name: &str) -> Result<&'a Json> {
    value
        .get(name)
        .ok_or_else(|| format!("missing JSON field {name}"))
}

/// Encode JSON strings, including all control bytes, and preserve valid UTF-8 text.
fn quoted(value: &str, output: &mut String) {
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{0}'..='\u{1f}' => output.push_str(&format!("\\u{:04x}", character as u32)),
            _ => output.push(character),
        }
    }
    output.push('"');
}
/// Serialize a bounded protocol value without invoking a dependency or shell.
fn encode(value: &Json, output: &mut String) {
    match value {
        Json::Null => output.push_str("null"),
        Json::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Json::Number(value) => output.push_str(value),
        Json::String(value) => quoted(value, output),
        Json::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                encode(value, output);
            }
            output.push(']');
        }
        Json::Object(fields) => {
            output.push('{');
            for (index, (name, value)) in fields.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                quoted(name, output);
                output.push(':');
                encode(value, output);
            }
            output.push('}');
        }
    }
}
/// Emit byte-accurate framing and flush promptly so editors cannot stall waiting for replies.
fn send(output: &mut impl Write, value: Json) -> Result<()> {
    let mut payload = String::new();
    encode(&value, &mut payload);
    write!(output, "Content-Length: {}\r\n\r\n{payload}", payload.len())
        .map_err(|error| error.to_string())?;
    output.flush().map_err(|error| error.to_string())
}
/// Reply with a standard JSON-RPC result preserving the request's identifier type.
fn respond(output: &mut impl Write, id: Json, result: Json) -> Result<()> {
    send(
        output,
        object([("jsonrpc", string("2.0")), ("id", id), ("result", result)]),
    )
}
/// Reply with a JSON-RPC error; parse/invalid-message failures use a null identifier.
fn send_error(output: &mut impl Write, id: Json, code: i64, message: &str) -> Result<()> {
    send(
        output,
        object([
            ("jsonrpc", string("2.0")),
            ("id", id),
            (
                "error",
                object([("code", number(code)), ("message", string(message))]),
            ),
        ]),
    )
}

#[derive(PartialEq, Eq)]
enum State {
    New,
    Running,
    Shutdown,
}
struct Document {
    source: String,
    version: i64,
}
struct Server {
    state: State,
    client_edits: capabilities::ClientEdits,
    documents: BTreeMap<String, Document>,
    published: BTreeMap<String, Json>,
}

impl Server {
    /// Validate JSON-RPC envelopes before lifecycle or document handlers inspect parameters.
    fn message(&mut self, message: Json, output: &mut impl Write) -> Result<bool> {
        let id = message.get("id").cloned();
        let valid_id = id
            .as_ref()
            .is_none_or(|id| matches!(id, Json::String(_)) || id.integer().is_ok());
        if message.get("jsonrpc").and_then(|value| value.string().ok()) != Some("2.0") || !valid_id
        {
            send_error(output, Json::Null, -32600, "invalid JSON-RPC envelope")?;
            return Ok(false);
        }
        let Some(method) = message.get("method").and_then(|value| value.string().ok()) else {
            if message.get("result").is_some() || message.get("error").is_some() {
                return Ok(false);
            }
            send_error(
                output,
                id.unwrap_or(Json::Null),
                -32600,
                "missing JSON-RPC method",
            )?;
            return Ok(false);
        };
        if method == "exit" && id.is_none() {
            return if self.state == State::Shutdown {
                Ok(true)
            } else {
                Err("exit received before shutdown".into())
            };
        }
        if self.state == State::Shutdown {
            if let Some(id) = id {
                send_error(output, id, -32600, "server has shut down")?;
            }
            return Ok(false);
        }
        if self.state == State::New && method != "initialize" {
            if let Some(id) = id {
                send_error(output, id, -32002, "server is not initialized")?;
            }
            return Ok(false);
        }
        let params = message.get("params").unwrap_or(&Json::Null);
        if let Some(id) = id {
            self.request(method, id, params, output)?;
        } else if self.state == State::Running {
            match self.notification(method, params) {
                Ok(messages) => {
                    for message in messages {
                        send(output, message)?;
                    }
                }
                Err(error) => send(
                    output,
                    object([
                        ("jsonrpc", string("2.0")),
                        ("method", string("window/logMessage")),
                        (
                            "params",
                            object([("type", number(1)), ("message", string(error))]),
                        ),
                    ]),
                )?,
            }
        }
        Ok(false)
    }
    /// Implement the required lifecycle without advertising unimplemented editor features.
    fn request(
        &mut self,
        method: &str,
        id: Json,
        params: &Json,
        output: &mut impl Write,
    ) -> Result<()> {
        match method {
            "initialize" if self.state == State::New => self.initialize(id, params, output),
            "textDocument/formatting" => match self.formatting(params) {
                Ok(result) => respond(output, id, result),
                Err((code, message)) => send_error(output, id, code, &message),
            },
            "textDocument/rename" | "textDocument/prepareRename" | "textDocument/codeAction" => {
                let result = match method {
                    "textDocument/rename" => self.rename(params),
                    "textDocument/prepareRename" => self.prepare_rename(params),
                    _ => self.code_actions(params),
                };
                match result {
                    Ok(result) => respond(output, id, result),
                    Err((code, message)) => send_error(output, id, code, &message),
                }
            }
            "textDocument/definition" | "textDocument/completion" | "textDocument/hover" => {
                match self.navigation(method, params) {
                    Ok(result) => respond(output, id, result),
                    Err(error) => send_error(output, id, -32602, &error),
                }
            }
            "initialize" => send_error(output, id, -32600, "initialize may only be requested once"),
            "shutdown" => {
                self.state = State::Shutdown;
                self.documents.clear();
                respond(output, id, Json::Null)
            }
            "exit" => send_error(output, id, -32600, "exit must be a notification"),
            _ => send_error(output, id, -32601, "method not found"),
        }
    }
    /// Initialize once with the implemented static capabilities and incremental UTF-16 sync.
    fn initialize(&mut self, id: Json, params: &Json, output: &mut impl Write) -> Result<()> {
        if !matches!(params, Json::Object(_)) {
            return send_error(output, id, -32602, "initialize params must be an object");
        }
        self.client_edits = capabilities::ClientEdits::initialize(params);
        self.state = State::Running;
        respond(
            output,
            id,
            object([
                (
                    "capabilities",
                    object([
                        ("positionEncoding", string("utf-16")),
                        ("definitionProvider", Json::Bool(true)),
                        ("hoverProvider", Json::Bool(true)),
                        ("documentFormattingProvider", Json::Bool(true)),
                        ("renameProvider", self.client_edits.rename_provider()),
                        ("codeActionProvider", self.client_edits.action_provider()),
                        (
                            "completionProvider",
                            object([
                                ("resolveProvider", Json::Bool(false)),
                                ("triggerCharacters", Json::Array(vec![string(".")])),
                            ]),
                        ),
                        (
                            "textDocumentSync",
                            object([("openClose", Json::Bool(true)), ("change", number(2))]),
                        ),
                    ]),
                ),
                (
                    "serverInfo",
                    object([
                        ("name", string("fern-rs")),
                        ("version", string(env!("CARGO_PKG_VERSION"))),
                    ]),
                ),
            ]),
        )
    }
    /// Apply document notifications; malformed edits leave all existing buffers intact.
    fn notification(&mut self, method: &str, params: &Json) -> Result<Vec<Json>> {
        let changed = match method {
            "textDocument/didOpen" => {
                self.open(params)?;
                true
            }
            "textDocument/didChange" => self.change(params)?,
            "textDocument/didClose" => {
                let uri = document_uri(params)?;
                self.documents.remove(uri);
                true
            }
            _ => false,
        };
        Ok(if changed { self.refresh() } else { Vec::new() })
    }
    /// Admit one source buffer under per-document, aggregate-byte and count limits.
    fn capacity(&self, uri: &str, source: &str) -> Result<()> {
        if source.len() > MAX_SOURCE {
            return Err("document exceeds 1 MiB source limit".into());
        }
        if !self.documents.contains_key(uri) && self.documents.len() >= MAX_DOCUMENTS {
            return Err("open-document limit exceeded".into());
        }
        let old = self
            .documents
            .get(uri)
            .map_or(0, |document| document.source.len());
        let total: usize = self
            .documents
            .values()
            .map(|document| document.source.len())
            .sum();
        if total - old + source.len() > MAX_TOTAL_SOURCE {
            return Err("open-document byte limit exceeded".into());
        }
        Ok(())
    }
    /// Open a versioned document and immediately publish source-located diagnostics.
    fn open(&mut self, params: &Json) -> Result<()> {
        let uri = document_uri(params)?;
        if self.documents.contains_key(uri) {
            return Err("document is already open".into());
        }
        self.unique_uri(uri)?;
        let document = field(params, "textDocument")?;
        let version = field(document, "version")?.integer()?;
        let source = field(document, "text")?.string()?;
        self.capacity(uri, source)?;
        self.documents.insert(
            uri.into(),
            Document {
                source: source.into(),
                version,
            },
        );
        Ok(())
    }
    /// Apply full or incremental changes sequentially, committing only a valid whole batch.
    fn change(&mut self, params: &Json) -> Result<bool> {
        let uri = document_uri(params)?;
        let document = self
            .documents
            .get(uri)
            .ok_or("change received for an unopened document")?;
        let version = field(field(params, "textDocument")?, "version")?.integer()?;
        if version <= document.version {
            return Ok(false);
        }
        let changes = field(params, "contentChanges")?.array()?;
        if changes.is_empty() || changes.len() > 256 {
            return Err("change batch must contain 1..256 edits".into());
        }
        let mut source = document.source.clone();
        for change in changes {
            apply_change(&mut source, change)?;
        }
        self.capacity(uri, &source)?;
        self.documents
            .insert(uri.into(), Document { source, version });
        Ok(true)
    }
}

impl Server {
    /// Reject aliasing open URIs so two buffers cannot compete for one source identity.
    fn unique_uri(&self, uri: &str) -> Result<()> {
        let Some(path) = file_path(uri)? else {
            return Ok(());
        };
        let Ok(identity) = modules::source_identity(&path) else {
            return Ok(());
        };
        for open in self.documents.keys() {
            if file_path(open)
                .ok()
                .flatten()
                .and_then(|p| modules::source_identity(&p).ok())
                .as_ref()
                == Some(&identity)
            {
                return Err("source file is already open under another URI".into());
            }
        }
        Ok(())
    }
    /// Recheck each open root with one coherent snapshot; one first error per graph is retained.
    fn refresh(&mut self) -> Vec<Json> {
        let mut sources = HashMap::new();
        let mut uris = BTreeMap::new();
        let mut reports = BTreeMap::new();
        for (uri, document) in &self.documents {
            reports.insert(uri.clone(), (document.source.clone(), None));
            if let Some(path) = file_path(uri)
                .ok()
                .flatten()
                .and_then(|p| modules::source_identity(&p).ok())
            {
                sources.insert(path.clone(), document.source.clone());
                uris.insert(path, uri.clone());
            }
        }
        for (uri, document) in &self.documents {
            let error = match file_path(uri).ok().flatten() {
                Some(path) => module_diagnostic(&path, &sources, &document.source),
                None => diagnostic(&document.source).map(|error| modules::SourceDiagnostic {
                    path: PathBuf::new(),
                    source: document.source.clone(),
                    diagnostic: error,
                }),
            };
            if let Some(error) = error {
                let target = if error.path.as_os_str().is_empty() {
                    uri.clone()
                } else {
                    uris.get(&error.path)
                        .cloned()
                        .unwrap_or_else(|| path_uri(&error.path))
                };
                let report = reports
                    .entry(target)
                    .or_insert_with(|| (error.source.clone(), None));
                if report.1.is_none() {
                    *report = (error.source, Some(error.diagnostic));
                }
            }
        }
        self.publications(reports)
    }
    /// Publish changed diagnostics and explicitly clear errors whose originating graph disappeared.
    fn publications(
        &mut self,
        reports: BTreeMap<String, (String, Option<crate::Diagnostic>)>,
    ) -> Vec<Json> {
        let mut messages = Vec::new();
        let mut next = BTreeMap::new();
        for uri in self
            .published
            .keys()
            .filter(|uri| !reports.contains_key(*uri))
        {
            messages.push(publish(uri, None, "", None));
        }
        for (uri, (source, error)) in reports {
            let version = self.documents.get(&uri).map(|document| document.version);
            let message = publish(&uri, version, &source, error);
            if self.published.get(&uri) != Some(&message) {
                messages.push(message.clone());
            }
            next.insert(uri, message);
        }
        self.published = next;
        messages
    }
}

/// Decode local file URIs; other document schemes keep single-buffer diagnostics.
fn file_path(uri: &str) -> Result<Option<PathBuf>> {
    let Some(rest) = uri.strip_prefix("file://") else {
        return Ok(None);
    };
    let path = if rest.starts_with("localhost/") {
        &rest["localhost".len()..]
    } else {
        rest
    };
    if !path.starts_with('/') || path.contains(['?', '#']) {
        return Err("expected a local absolute file URI".into());
    }
    let mut bytes = Vec::new();
    let mut input = path.bytes();
    while let Some(byte) = input.next() {
        if byte == b'%' {
            let hex = |b: u8| (b as char).to_digit(16).map(|n| n as u8);
            let high = input
                .next()
                .and_then(hex)
                .ok_or("invalid percent escape in file URI")?;
            let low = input
                .next()
                .and_then(hex)
                .ok_or("invalid percent escape in file URI")?;
            bytes.push(high * 16 + low);
        } else {
            bytes.push(byte);
        }
    }
    if bytes.contains(&0) {
        return Err("file URI contains a null byte".into());
    }
    Ok(Some(PathBuf::from(
        String::from_utf8(bytes).map_err(|_| "file URI is not UTF-8")?,
    )))
}

/// Encode paths for diagnostics from imported files that have no client-provided URI.
fn path_uri(path: &Path) -> String {
    use std::fmt::Write as _;
    let mut uri = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
            uri.push(byte as char);
        } else {
            write!(&mut uri, "%{byte:02X}").expect("writing a String cannot fail");
        }
    }
    uri
}

/// Use loader source maps for parser, visibility and checker errors from imported buffers.
fn module_diagnostic(
    path: &Path,
    sources: &HashMap<PathBuf, String>,
    source: &str,
) -> Option<modules::SourceDiagnostic> {
    let loaded = match modules::load_with_sources(path, sources) {
        Ok(loaded) => loaded,
        Err(error) => {
            return Some(error.location.map(|location| *location).unwrap_or_else(|| {
                modules::SourceDiagnostic {
                    path: modules::source_identity(path).unwrap_or_else(|_| path.to_owned()),
                    source: source.into(),
                    diagnostic: crate::Diagnostic::new(Span::default(), error.message),
                }
            }));
        }
    };
    check::check_library(&loaded.program)
        .err()
        .and_then(|error| loaded.locate(error))
}

/// Require a bounded URI; document contents remain in memory regardless of URI scheme.
fn document_uri(params: &Json) -> Result<&str> {
    let uri = field(field(params, "textDocument")?, "uri")?.string()?;
    if uri.is_empty() || uri.len() > 4096 {
        return Err("document URI must contain 1..4096 bytes".into());
    }
    Ok(uri)
}
/// Apply one validated UTF-16 range or full replacement, preserving the previous buffer on error.
fn apply_change(source: &mut String, change: &Json) -> Result<()> {
    let text = field(change, "text")?.string()?;
    let (start, end) = if let Some(range) = change.get("range") {
        (
            byte_position(source, field(range, "start")?)?,
            byte_position(source, field(range, "end")?)?,
        )
    } else {
        (0, source.len())
    };
    if end < start {
        return Err("change range ends before its start".into());
    }
    if let Some(length) = change.get("rangeLength") {
        let length = usize::try_from(length.integer()?).map_err(|_| "invalid rangeLength")?;
        if source[start..end].encode_utf16().count() != length {
            return Err("rangeLength does not match document text".into());
        }
    }
    if text.len() > MAX_SOURCE || source.len() - (end - start) + text.len() > MAX_SOURCE {
        return Err("changed document exceeds 1 MiB source limit".into());
    }
    source.replace_range(start..end, text);
    Ok(())
}
/// Map a UTF-16 editor position to a byte boundary, rejecting surrogate splits and invalid lines.
fn byte_position(source: &str, position: &Json) -> Result<usize> {
    let line = usize::try_from(field(position, "line")?.integer()?).map_err(|_| "negative line")?;
    let column = usize::try_from(field(position, "character")?.integer()?)
        .map_err(|_| "negative character")?;
    let mut start = 0;
    for _ in 0..line {
        start += source[start..]
            .find('\n')
            .ok_or("position line is outside document")?
            + 1;
    }
    let end = source[start..]
        .find('\n')
        .map_or(source.len(), |end| start + end);
    let text = source[start..end]
        .strip_suffix('\r')
        .unwrap_or_else(|| &source[start..end]);
    let mut units = 0;
    for (offset, character) in text.char_indices() {
        if units == column {
            return Ok(start + offset);
        }
        units += character.len_utf16();
        if units > column {
            return Err("position splits a UTF-16 surrogate pair".into());
        }
    }
    if units == column {
        Ok(start + text.len())
    } else {
        Err("position character is outside document line".into())
    }
}
/// Convert checked byte spans into LSP UTF-16 coordinates, clamping end-of-line CRLF positions.
fn position(source: &str, offset: usize) -> Json {
    let mut offset = offset.min(source.len());
    while !source.is_char_boundary(offset) {
        offset -= 1;
    }
    let prefix = &source[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let tail = prefix
        .rsplit('\n')
        .next()
        .unwrap_or("")
        .strip_suffix('\r')
        .unwrap_or_else(|| prefix.rsplit('\n').next().unwrap_or(""));
    object([
        ("line", number(line)),
        ("character", number(tail.encode_utf16().count())),
    ])
}
/// Validate complete editor libraries without introducing source-visible names.
fn diagnostic(source: &str) -> Option<crate::Diagnostic> {
    let program = match parse::parse(source) {
        Ok(program) => program,
        Err(error) => return Some(error),
    };
    check::check_library(&program).err()
}

/// Publish one first-error diagnostic or an empty list to clear stale editor squiggles.
fn publish(
    uri: &str,
    version: Option<i64>,
    source: &str,
    error: Option<crate::Diagnostic>,
) -> Json {
    let diagnostics = error
        .map(|error| {
            object([
                (
                    "range",
                    object([
                        ("start", position(source, error.span.start)),
                        (
                            "end",
                            position(source, error.span.end.max(error.span.start)),
                        ),
                    ]),
                ),
                ("severity", number(1)),
                ("source", string("fern-rs")),
                ("message", string(error.message)),
            ])
        })
        .into_iter()
        .collect();
    let mut params = BTreeMap::from([
        ("uri".into(), string(uri)),
        ("diagnostics".into(), Json::Array(diagnostics)),
    ]);
    if let Some(version) = version {
        params.insert("version".into(), number(version));
    }
    object([
        ("jsonrpc", string("2.0")),
        ("method", string("textDocument/publishDiagnostics")),
        ("params", Json::Object(params)),
    ])
}
