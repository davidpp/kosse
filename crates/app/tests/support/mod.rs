//! Shared by the binary-level suites: an owned `kosse` child and a raw HTTP/1.1 client, so
//! tests control every header (forged `Host`, `Origin`, content types) exactly.
#![expect(
    clippy::expect_used,
    clippy::panic,
    dead_code,
    reason = "test helpers fail the test by panicking; each suite uses a subset"
)]

use std::{
    fmt::Write as _,
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpStream},
    path::Path,
    process::{Child, ChildStderr, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

use kosse_core::{contract::Session, model::Snapshot};

pub const EXIT_TIMEOUT: Duration = Duration::from_secs(10);
/// The configuration directory of every spawned `kosse` unless a test gives its own: it never
/// exists and cannot be created, so tests never read or write the user's real profiles.
pub const NO_CONFIG: &str = "/var/empty/kosse-tests";

/// A spawned `kosse` that is killed (by this handle only) if the test ends early.
pub struct OwnedChild(pub Child);

impl OwnedChild {
    pub fn spawn(args: &[&str]) -> Self {
        Self::spawn_configured(Path::new(NO_CONFIG), args)
    }

    /// With `config_dir` as `KOSSE_CONFIG_DIR`.
    pub fn spawn_configured(config_dir: &Path, args: &[&str]) -> Self {
        let child = Command::new(env!("CARGO_BIN_EXE_kosse"))
            .env("KOSSE_CONFIG_DIR", config_dir)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn kosse");
        Self(child)
    }

    pub fn take_stderr(&mut self) -> BufReader<ChildStderr> {
        BufReader::new(self.0.stderr.take().expect("piped stderr"))
    }

    pub fn wait_with_timeout(&mut self) -> ExitStatus {
        let deadline = Instant::now() + EXIT_TIMEOUT;
        loop {
            if let Some(status) = self.0.try_wait().expect("poll child") {
                return status;
            }
            assert!(Instant::now() < deadline, "kosse did not exit in time");
            thread::sleep(Duration::from_millis(20));
        }
    }

    /// Runs to completion (bounded by [`EXIT_TIMEOUT`]) and returns the exit status and stderr.
    pub fn run(args: &[&str]) -> (ExitStatus, String) {
        let mut child = Self::spawn(args);
        let mut stderr = child.take_stderr();
        let status = child.wait_with_timeout();
        let mut output = String::new();
        stderr.read_to_string(&mut output).expect("read stderr");
        (status, output)
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

pub struct Server {
    pub child: OwnedChild,
    pub addr: SocketAddr,
    stderr: Option<BufReader<ChildStderr>>,
}

impl Server {
    /// `serve --no-open --port 0` plus `extra` arguments. Unless `extra` says otherwise, the
    /// server probes nothing (`--probe-only=`) and may signal nothing (`--act-only=`): tests
    /// never probe or signal the machine's real services.
    pub fn start_with(extra: &[&str]) -> Self {
        Self::start_configured(Path::new(NO_CONFIG), extra)
    }

    /// [`Server::start_with`] with `config_dir` as `KOSSE_CONFIG_DIR`.
    pub fn start_configured(config_dir: &Path, extra: &[&str]) -> Self {
        let mut args = vec!["serve", "--no-open", "--port", "0"];
        args.extend_from_slice(extra);
        if !extra.iter().any(|arg| arg.starts_with("--probe-only")) {
            args.push("--probe-only=");
        }
        if !extra.iter().any(|arg| arg.starts_with("--act-only")) {
            args.push("--act-only=");
        }
        let mut child = OwnedChild::spawn_configured(config_dir, &args);
        let mut stderr = child.take_stderr();
        let mut line = String::new();
        loop {
            line.clear();
            stderr.read_line(&mut line).expect("read startup line");
            assert!(!line.is_empty(), "kosse exited before listening");
            if line.contains("listening on") {
                break;
            }
        }
        let addr = line
            .split("listening on http://")
            .nth(1)
            .and_then(|rest| rest.trim().trim_end_matches('/').parse().ok())
            .unwrap_or_else(|| panic!("unexpected startup line: {line:?}"));
        Self {
            child,
            addr,
            stderr: Some(stderr),
        }
    }

    pub fn start() -> Self {
        Self::start_with(&[])
    }

    /// `127.0.0.1:<port>`, the authority the server answers for.
    pub fn authority(&self) -> String {
        self.addr.to_string()
    }

    pub fn origin(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// A request with the server's own `Host` and nothing else.
    pub fn get(&self, path: &str) -> HttpResponse {
        self.send(&RawRequest::new("GET", path).host(&self.authority()))
    }

    pub fn send(&self, request: &RawRequest) -> HttpResponse {
        request.send(self.addr)
    }

    pub fn token(&self) -> String {
        let response = self.get("/api/session");
        assert_eq!(response.status, 200, "{}", response.text());
        let session: Session = serde_json::from_slice(&response.body).expect("session json");
        session.token
    }

    /// A same-origin JSON `POST`, as the dashboard sends it.
    pub fn post_json(&self, path: &str, token: &str, body: &str) -> HttpResponse {
        self.send(
            &RawRequest::new("POST", path)
                .host(&self.authority())
                .header("Origin", &self.origin())
                .header("Content-Type", "application/json")
                .header("X-Kosse-Token", token)
                .body(body),
        )
    }

    pub fn snapshot(&self) -> Snapshot {
        let response = self.get("/api/snapshot");
        assert_eq!(response.status, 200, "{}", response.text());
        serde_json::from_slice(&response.body).expect("snapshot json")
    }

    /// Sends SIGTERM to the child this test spawned and waits for it to exit.
    pub fn terminate(mut self) -> (ExitStatus, String) {
        let status = Command::new("kill")
            .args(["-TERM", &self.child.0.id().to_string()])
            .status()
            .expect("run kill");
        assert!(status.success(), "kill -TERM failed");
        let exit = self.child.wait_with_timeout();
        (exit, self.rest_of_stderr())
    }

    /// Everything the server wrote to stderr after its startup line, once it has exited.
    fn rest_of_stderr(&mut self) -> String {
        let mut rest = String::new();
        if let Some(mut stderr) = self.stderr.take() {
            stderr.read_to_string(&mut rest).expect("read stderr");
        }
        rest
    }
}

/// One HTTP/1.1 request with exactly the given headers (plus `Connection: close` and a
/// `Content-Length` when there is a body).
#[derive(Clone)]
pub struct RawRequest {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
    chunked: Option<String>,
    version: &'static str,
}

impl RawRequest {
    pub fn new(method: &str, target: &str) -> Self {
        Self {
            method: method.to_owned(),
            target: target.to_owned(),
            headers: Vec::new(),
            body: None,
            chunked: None,
            version: "HTTP/1.1",
        }
    }

    pub fn host(self, host: &str) -> Self {
        self.header("Host", host)
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }

    /// Sends `body` with `Transfer-Encoding: chunked` (no `Content-Length`), in one chunk.
    pub fn chunked(mut self, body: &str) -> Self {
        self.headers
            .push(("Transfer-Encoding".to_owned(), "chunked".to_owned()));
        self.chunked = Some(body.to_owned());
        self
    }

    pub fn body(mut self, body: &str) -> Self {
        self.body = Some(body.to_owned());
        self
    }

    pub fn http10(mut self) -> Self {
        self.version = "HTTP/1.0";
        self
    }

    fn bytes(&self) -> Vec<u8> {
        let mut head = format!("{} {} {}\r\n", self.method, self.target, self.version);
        for (name, value) in &self.headers {
            let _ = write!(head, "{name}: {value}\r\n");
        }
        head.push_str("Connection: close\r\n");
        if let Some(body) = &self.body {
            let _ = write!(head, "Content-Length: {}\r\n", body.len());
        }
        head.push_str("\r\n");
        let mut bytes = head.into_bytes();
        if let Some(body) = &self.body {
            bytes.extend_from_slice(body.as_bytes());
        }
        if let Some(body) = &self.chunked {
            bytes.extend_from_slice(format!("{:x}\r\n{body}\r\n0\r\n\r\n", body.len()).as_bytes());
        }
        bytes
    }

    pub fn send(&self, addr: SocketAddr) -> HttpResponse {
        let mut stream = TcpStream::connect(addr).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(15)))
            .expect("read timeout");
        stream.write_all(&self.bytes()).expect("write request");
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).expect("read response");
        HttpResponse::parse(&raw)
    }
}

pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn parse(raw: &[u8]) -> Self {
        let split = raw
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap_or_else(|| panic!("no header terminator in {raw:?}"));
        let head = std::str::from_utf8(&raw[..split]).expect("utf-8 headers");
        let mut lines = head.split("\r\n");
        let status = lines
            .next()
            .and_then(|line| line.split(' ').nth(1))
            .and_then(|code| code.parse().ok())
            .expect("status line");
        let headers: Vec<(String, String)> = lines
            .filter_map(|line| line.split_once(':'))
            .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_owned()))
            .collect();
        let mut body = raw[split + 4..].to_vec();
        if headers
            .iter()
            .any(|(name, value)| name == "transfer-encoding" && value == "chunked")
        {
            body = dechunk(&body);
        }
        Self {
            status,
            headers,
            body,
        }
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// The `error` code of an `ApiError` body.
    pub fn error_code(&self) -> String {
        let value: serde_json::Value = serde_json::from_slice(&self.body)
            .unwrap_or_else(|_| panic!("not an API error: {}", self.text()));
        value["error"]
            .as_str()
            .unwrap_or_else(|| panic!("not an API error: {}", self.text()))
            .to_owned()
    }
}

fn dechunk(mut raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(end) = raw.windows(2).position(|window| window == b"\r\n") {
        let size = std::str::from_utf8(&raw[..end])
            .ok()
            .and_then(|line| usize::from_str_radix(line.trim(), 16).ok())
            .unwrap_or(0);
        raw = &raw[end + 2..];
        if size == 0 || raw.len() < size {
            break;
        }
        out.extend_from_slice(&raw[..size]);
        raw = raw.get(size + 2..).unwrap_or_default();
    }
    out
}

/// A Server-Sent Events subscription over a raw connection.
pub struct EventStream {
    stream: TcpStream,
    pending: Vec<u8>,
    decoded: Vec<u8>,
    pub status: u16,
    pub headers: String,
}

impl EventStream {
    pub fn open(server: &Server) -> Self {
        let mut stream = TcpStream::connect(server.addr).expect("connect");
        write!(
            stream,
            "GET /api/events HTTP/1.1\r\nHost: {}\r\nAccept: text/event-stream\r\n\r\n",
            server.authority()
        )
        .expect("write request");
        stream
            .set_read_timeout(Some(Duration::from_millis(200)))
            .expect("read timeout");
        let mut this = Self {
            stream,
            pending: Vec::new(),
            decoded: Vec::new(),
            status: 0,
            headers: String::new(),
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while !this.pending.windows(4).any(|window| window == b"\r\n\r\n") {
            assert!(Instant::now() < deadline, "no response head");
            this.fill();
        }
        let split = this
            .pending
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .expect("head");
        this.headers = String::from_utf8_lossy(&this.pending[..split]).to_ascii_lowercase();
        this.status = this
            .headers
            .split(' ')
            .nth(1)
            .and_then(|code| code.parse().ok())
            .expect("status");
        this.pending.drain(..split + 4);
        this
    }

    fn fill(&mut self) -> bool {
        let mut buffer = [0; 16 * 1024];
        match self.stream.read(&mut buffer) {
            Ok(0) => false,
            Ok(read) => {
                self.pending.extend_from_slice(&buffer[..read]);
                true
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                true
            }
            Err(_) => false,
        }
    }

    /// Moves complete chunks from `pending` into `decoded`.
    fn decode(&mut self) {
        while let Some(end) = self.pending.windows(2).position(|window| window == b"\r\n") {
            let size = std::str::from_utf8(&self.pending[..end])
                .ok()
                .and_then(|line| usize::from_str_radix(line.trim(), 16).ok())
                .expect("chunk size");
            if self.pending.len() < end + 2 + size + 2 {
                return;
            }
            self.decoded
                .extend_from_slice(&self.pending[end + 2..end + 2 + size]);
            self.pending.drain(..end + 2 + size + 2);
        }
    }

    /// The next `snapshot` event within `timeout`; `None` if the stream ended or stayed silent.
    pub fn next_snapshot(&mut self, timeout: Duration) -> Option<Snapshot> {
        let deadline = Instant::now() + timeout;
        loop {
            self.decode();
            while let Some(end) = self.decoded.windows(2).position(|window| window == b"\n\n") {
                let event: Vec<u8> = self.decoded.drain(..end + 2).collect();
                let event = String::from_utf8_lossy(&event).into_owned();
                if !event.lines().any(|line| line == "event: snapshot") {
                    continue; // keep-alive comment
                }
                let data: String = event
                    .lines()
                    .filter_map(|line| line.strip_prefix("data: "))
                    .collect();
                return Some(serde_json::from_str(&data).expect("snapshot event json"));
            }
            if Instant::now() >= deadline || !self.fill() {
                return None;
            }
        }
    }

    /// Raw decoded text received so far (for leak checks).
    pub fn raw_text(&self) -> String {
        String::from_utf8_lossy(&self.decoded).into_owned()
    }
}

/// Polls `condition` every 100 ms until it holds or `timeout` passes.
/// The requests in `received` (a fixture's recorded connections) that the Kossé instance
/// `instance` sent. Any other Kossé on the machine, such as the developer's own, probes the same
/// fixture projects; its requests carry its own instance ID and are not counted.
pub fn sent_by(received: Vec<String>, instance: &str) -> Vec<String> {
    let line = format!("\r\n{}: {instance}\r\n", kosse_core::probe::INSTANCE_HEADER);
    received
        .into_iter()
        .filter(|request| request.to_ascii_lowercase().contains(&line))
        .collect()
}

pub fn eventually<T>(timeout: Duration, mut condition: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(value) = condition() {
            return Some(value);
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

/// One GFM table of a command's Markdown output: the header cells and each row's cells,
/// unescaped (`\|`, `\\`, `` \` ``, `\[`, `\]`, `\<` and `\>` back to the character).
#[derive(Debug)]
pub struct MarkdownTable {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl MarkdownTable {
    /// The cells of `row` by header name.
    pub fn cell<'a>(&self, row: &'a [String], header: &str) -> &'a str {
        let index = self
            .headers
            .iter()
            .position(|name| name == header)
            .unwrap_or_else(|| panic!("no column {header} in {:?}", self.headers));
        &row[index]
    }

    /// The rows whose `header` cell is `value`.
    pub fn rows_where(&self, header: &str, value: &str) -> Vec<&Vec<String>> {
        self.rows
            .iter()
            .filter(|row| self.cell(row, header) == value)
            .collect()
    }
}

/// Every table in `text`, checked the way a GFM parser reads them: a header line, a delimiter
/// line of `---` cells, then rows with exactly the header's column count, each line starting
/// and ending with an unescaped `|`. Panics on anything else that starts with `|`, and on a
/// line between rows that is not one: a hostile cell must not end a table early or add a row.
pub fn markdown_tables(text: &str) -> Vec<MarkdownTable> {
    let mut tables = Vec::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        if !line.starts_with('|') {
            continue;
        }
        let headers = split_row(line);
        let delimiter = split_row(
            lines
                .next()
                .unwrap_or_else(|| panic!("no delimiter after {line:?}")),
        );
        assert_eq!(delimiter.len(), headers.len(), "delimiter of {line:?}");
        assert!(
            delimiter
                .iter()
                .all(|cell| cell.len() >= 3 && cell.chars().all(|c| c == '-')),
            "not a delimiter row under {line:?}: {delimiter:?}"
        );
        let mut rows = Vec::new();
        while let Some(row) = lines.next_if(|next| !next.is_empty()) {
            let cells = split_row(row);
            assert_eq!(cells.len(), headers.len(), "row {row:?} under {headers:?}");
            rows.push(cells);
        }
        tables.push(MarkdownTable { headers, rows });
    }
    tables
}

fn split_row(line: &str) -> Vec<String> {
    assert!(
        line.starts_with('|') && line.ends_with('|') && !line.ends_with("\\|"),
        "not a table row: {line:?}"
    );
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut escaped = false;
    for character in line[1..].chars() {
        match character {
            _ if escaped => {
                assert!(
                    matches!(character, '|' | '\\' | '`' | '[' | ']' | '<' | '>'),
                    "unexpected escape \\{character} in {line:?}"
                );
                cell.push(character);
                escaped = false;
            }
            '\\' => escaped = true,
            '`' => panic!("unescaped backtick (a code span) in {line:?}"),
            '[' | ']' | '<' | '>' => panic!("unescaped {character} (a link or HTML) in {line:?}"),
            '|' => cells.push(std::mem::take(&mut cell).trim().to_owned()),
            _ => cell.push(character),
        }
    }
    cells
}
