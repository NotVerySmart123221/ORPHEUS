//! Optional, loopback-only HTTP view of native server state.

use std::{
    io::{self, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener},
    sync::{Arc, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::{
    game_debug::Snapshot,
    service_tcp::{PeerStream, PeerWorkers, accept},
};

const ACCEPT_POLL: Duration = Duration::from_millis(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_PEERS: usize = 8;
const MAX_REQUEST_BYTES: usize = 8192;
const HTML: &str = include_str!("debug_web/index.html");
const CSS: &str = include_str!("debug_web/debug.css");
const SCRIPT: &str = include_str!("debug_web/debug.js");

pub(crate) struct DebugHttp {
    addr: SocketAddr,
    stop: mpsc::Sender<()>,
    worker: Option<JoinHandle<io::Result<()>>>,
}

impl DebugHttp {
    pub(crate) fn bind(
        port: u16,
        geometry: Vec<u8>,
        state: impl Fn() -> io::Result<Snapshot> + Send + Sync + 'static,
    ) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))?;
        listener.set_nonblocking(true)?;
        let addr = listener.local_addr()?;
        let host = addr.to_string();
        let endpoint = Arc::new(Endpoint {
            origin: format!("http://{host}"),
            host,
            geometry,
            state: Box::new(state),
        });
        let (stop, shutdown) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("debug-http".into())
            .spawn(move || serve(listener, endpoint, shutdown))?;
        Ok(Self {
            addr,
            stop,
            worker: Some(worker),
        })
    }

    pub(crate) fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub(crate) fn stop(&mut self) -> io::Result<()> {
        if let Some(worker) = self.worker.take() {
            let _ = self.stop.send(());
            worker
                .join()
                .map_err(|_| io::Error::other("debug HTTP listener thread panicked"))??;
        }
        Ok(())
    }
}

impl Drop for DebugHttp {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

struct Endpoint {
    host: String,
    origin: String,
    geometry: Vec<u8>,
    state: Box<dyn Fn() -> io::Result<Snapshot> + Send + Sync>,
}

fn serve(
    listener: TcpListener,
    endpoint: Arc<Endpoint>,
    shutdown: mpsc::Receiver<()>,
) -> io::Result<()> {
    let mut peers = PeerWorkers::default();
    while matches!(shutdown.try_recv(), Err(mpsc::TryRecvError::Empty)) {
        peers.reap();
        if peers.len() < MAX_PEERS {
            if let Some((socket, _)) = accept(&listener)? {
                let endpoint = Arc::clone(&endpoint);
                let _ = peers.spawn("debug-http-peer", socket, move |socket| {
                    let _ = serve_peer(socket, &endpoint);
                });
                continue;
            }
        }
        if !matches!(
            shutdown.recv_timeout(ACCEPT_POLL),
            Err(mpsc::RecvTimeoutError::Timeout)
        ) {
            break;
        }
    }
    Ok(())
}

fn serve_peer(mut socket: PeerStream, endpoint: &Endpoint) -> io::Result<()> {
    socket.set_deadline(Some(Instant::now() + REQUEST_TIMEOUT));
    let mut buffer = [0; MAX_REQUEST_BYTES];
    let mut filled = 0_usize;
    loop {
        let read = socket.read(&mut buffer[filled..])?;
        if read == 0 {
            return Ok(());
        }
        let scan_start = filled.saturating_sub(3);
        filled += read;
        if buffer[scan_start..filled]
            .windows(4)
            .any(|bytes| bytes == b"\r\n\r\n")
        {
            break;
        }
        if filled == buffer.len() {
            return error_response(&mut socket, "431 Request Header Fields Too Large");
        }
    }

    let mut headers = [httparse::EMPTY_HEADER; 32];
    let mut request = httparse::Request::new(&mut headers);
    match request.parse(&buffer[..filled]) {
        Ok(httparse::Status::Complete(end)) if end == filled => {}
        Err(httparse::Error::TooManyHeaders) => {
            return error_response(&mut socket, "431 Request Header Fields Too Large");
        }
        _ => return error_response(&mut socket, "400 Bad Request"),
    }

    if !allowed_headers(request.headers, endpoint) {
        return error_response(&mut socket, "400 Bad Request");
    }
    if request.method != Some("GET") {
        return error_response(&mut socket, "405 Method Not Allowed");
    }
    match request.path {
        Some("/") => respond(
            &mut socket,
            "200 OK",
            "text/html; charset=utf-8",
            HTML.as_bytes(),
        ),
        Some("/debug.css") => respond(
            &mut socket,
            "200 OK",
            "text/css; charset=utf-8",
            CSS.as_bytes(),
        ),
        Some("/debug.js") => respond(
            &mut socket,
            "200 OK",
            "text/javascript; charset=utf-8",
            SCRIPT.as_bytes(),
        ),
        Some("/api/debug/v1/geometry") => respond(
            &mut socket,
            "200 OK",
            "application/json",
            &endpoint.geometry,
        ),
        Some("/api/debug/v1/state") => {
            let Ok(state) = (endpoint.state)() else {
                return error_response(&mut socket, "503 Service Unavailable");
            };
            let body = serde_json::to_vec(&state).map_err(io::Error::other)?;
            respond(&mut socket, "200 OK", "application/json", &body)
        }
        _ => error_response(&mut socket, "404 Not Found"),
    }
}

fn allowed_headers(headers: &[httparse::Header<'_>], endpoint: &Endpoint) -> bool {
    let mut host_seen = false;
    let mut origin_seen = false;
    let mut content_length_seen = false;
    for header in headers {
        if header.name.eq_ignore_ascii_case("host") {
            if host_seen || header.value != endpoint.host.as_bytes() {
                return false;
            }
            host_seen = true;
        } else if header.name.eq_ignore_ascii_case("origin") {
            if origin_seen || header.value != endpoint.origin.as_bytes() {
                return false;
            }
            origin_seen = true;
        } else if header.name.eq_ignore_ascii_case("content-length") {
            if content_length_seen || header.value != b"0" {
                return false;
            }
            content_length_seen = true;
        } else if header.name.eq_ignore_ascii_case("transfer-encoding") {
            return false;
        }
    }
    host_seen
}

fn error_response(socket: &mut PeerStream, status: &str) -> io::Result<()> {
    respond(
        socket,
        status,
        "text/plain; charset=utf-8",
        status.as_bytes(),
    )
}

fn respond(
    socket: &mut PeerStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> io::Result<()> {
    let allow = if status == "405 Method Not Allowed" {
        "Allow: GET\r\n"
    } else {
        ""
    };
    write!(
        socket,
        "HTTP/1.1 {status}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         Content-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'\r\n\
         X-Content-Type-Options: nosniff\r\n\
         Cross-Origin-Resource-Policy: same-origin\r\n\
         Referrer-Policy: no-referrer\r\n\
         {allow}\
         Connection: close\r\n\r\n",
        body.len(),
    )?;
    socket.write_all(body)
}

#[cfg(test)]
mod tests {
    use std::net::TcpStream;

    use super::*;

    fn request(addr: SocketAddr, raw: &str) -> String {
        let mut socket = TcpStream::connect(addr).unwrap();
        socket.set_read_timeout(Some(REQUEST_TIMEOUT)).unwrap();
        socket.write_all(raw.as_bytes()).unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).unwrap();
        response
    }

    #[test]
    fn serves_loopback_assets_geometry_and_live_state() {
        let mut http = DebugHttp::bind(0, br#"{"map":"0000000000000000"}"#.to_vec(), || {
            Ok(Snapshot {
                tick: 42,
                rooms: Vec::new(),
                truncated: false,
            })
        })
        .unwrap();
        let addr = http.addr();
        assert_eq!(addr.ip(), Ipv4Addr::LOCALHOST);

        let response = request(addr, &format!("GET / HTTP/1.1\r\nHost: {addr}\r\n\r\n"));
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(response.contains("Content-Security-Policy:"));
        assert!(response.contains("Cache-Control: no-store"));
        assert!(response.contains("ORPHEUS"));

        for path in ["/debug.css", "/debug.js"] {
            let response = request(
                addr,
                &format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\n\r\n"),
            );
            assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        }
        for (route, expected) in [
            ("geometry", r#"{"map":"0000000000000000"}"#),
            ("state", r#"{"tick":42,"rooms":[],"truncated":false}"#),
        ] {
            let response = request(
                addr,
                &format!(
                    "GET /api/debug/v1/{route} HTTP/1.1\r\nHost: {addr}\r\nOrigin: http://{addr}\r\n\r\n"
                ),
            );
            assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
            assert!(response.contains("Content-Type: application/json"));
            let (_, body) = response.split_once("\r\n\r\n").unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(body).unwrap(),
                serde_json::from_str::<serde_json::Value>(expected).unwrap()
            );
        }

        http.stop().unwrap();
        assert!(TcpStream::connect(addr).is_err());
        http.stop().unwrap();
    }

    #[test]
    fn caps_request_bytes_and_header_count() {
        let http = DebugHttp::bind(0, Vec::new(), || unreachable!()).unwrap();
        let addr = http.addr();
        let mut request_line = format!("GET / HTTP/1.1\r\nHost: {addr}\r\nLong: ");
        request_line.push_str(&"x".repeat(MAX_REQUEST_BYTES - request_line.len()));
        let too_many = format!(
            "GET / HTTP/1.1\r\nHost: {addr}\r\n{}\r\n",
            "X: x\r\n".repeat(32)
        );
        for raw in [request_line, too_many] {
            let response = request(addr, &raw);
            assert!(response.starts_with("HTTP/1.1 431"), "{response}");
        }
    }

    #[test]
    fn refuses_untrusted_requests_and_hides_snapshot_errors() {
        let http = DebugHttp::bind(0, Vec::new(), || {
            Err(io::Error::other("private simulation error"))
        })
        .unwrap();
        let addr = http.addr();
        let cases = [
            (format!("POST / HTTP/1.1\r\nHost: {addr}\r\n\r\n"), "405"),
            ("GET / HTTP/1.1\r\nHost: example.test\r\n\r\n".into(), "400"),
            (
                format!("GET / HTTP/1.1\r\nHost: {addr}\r\nHost: {addr}\r\n\r\n"),
                "400",
            ),
            (
                format!("GET / HTTP/1.1\r\nHost: {addr}\r\nOrigin: https://example.test\r\n\r\n"),
                "400",
            ),
            (
                format!("GET / HTTP/1.1\r\nHost: {addr}\r\nContent-Length: 1\r\n\r\n"),
                "400",
            ),
            (
                format!("GET / HTTP/1.1\r\nHost: {addr}\r\nTransfer-Encoding: chunked\r\n\r\n"),
                "400",
            ),
            (
                format!("GET /unknown HTTP/1.1\r\nHost: {addr}\r\n\r\n"),
                "404",
            ),
            (
                format!("GET /api/debug/v1/state HTTP/1.1\r\nHost: {addr}\r\n\r\n"),
                "503",
            ),
        ];
        for (raw, status) in cases {
            let response = request(addr, &raw);
            assert!(
                response.starts_with(&format!("HTTP/1.1 {status}")),
                "{response}"
            );
            assert!(!response.contains("private simulation error"));
        }
    }
}
