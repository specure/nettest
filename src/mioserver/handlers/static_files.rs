//! Embedded browser client, served on the measurement port(s).
//!
//! The measurement handshake starts with an HTTP request: RMBT clients send
//! `Upgrade: RMBT`, browsers running a test send `Upgrade: websocket`. Any
//! other `GET`/`HEAD` is a browser asking for the page itself, so the worker
//! answers it from the files embedded here and closes the connection. Page and
//! WebSocket therefore share one origin and one port — the client simply
//! connects back to `location.host`.
//!
//! `dist/` is produced by `scripts/build-web.sh`. Files are stored
//! gzip-compressed (`*.gz`) and sent with `Content-Encoding: gzip`; brotli is
//! not an option because browsers only accept `br` over HTTPS.

use include_dir::{include_dir, Dir, File};
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::io::Read;
use std::net::SocketAddr;

use crate::mioserver::server::ServerConfig;

// Embed the dist directory at compile time
static STATIC_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/dist");

/// Content-Security-Policy for the page. `wasm-unsafe-eval` is required to
/// compile the WebAssembly module; `connect-src` allows the measurement
/// WebSocket (ws/wss to any host, so a user may pick another server) and the
/// optional result upload to a control server.
const CSP: &str = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; \
style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; \
connect-src 'self' ws: wss: https: http:; base-uri 'none'; form-action 'none'; \
frame-ancestors 'none'";

lazy_static! {
    /// Strong ETag per embedded file, computed once from its stored bytes.
    static ref ETAGS: HashMap<&'static str, String> = {
        let mut map = HashMap::new();
        collect_etags(&STATIC_DIR, &mut map);
        map
    };
}

fn collect_etags(dir: &'static Dir<'static>, map: &mut HashMap<&'static str, String>) {
    for file in dir.files() {
        if let Some(path) = file.path().to_str() {
            map.insert(path, format!("\"{:016x}\"", fnv1a64(file.contents())));
        }
    }
    for sub in dir.dirs() {
        collect_etags(sub, map);
    }
}

fn fnv1a64(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// True for a plain `GET`/`HEAD` that does not ask for a protocol upgrade,
/// i.e. a browser loading the page rather than a client starting a test.
pub fn is_static_file_request(request: &str) -> bool {
    let method = request.split_whitespace().next().unwrap_or("");
    if method != "GET" && method != "HEAD" {
        return false;
    }
    header_value(request, "upgrade").is_none()
}

/// Request target of the first line, without query string or fragment.
pub fn parse_http_path(request: &str) -> Option<String> {
    let first_line = request.lines().next()?;
    let mut parts = first_line.split_whitespace();
    let method = parts.next()?;
    if method != "GET" && method != "HEAD" {
        return None;
    }
    let target = parts.next()?;
    let path = target.split(['?', '#']).next().unwrap_or("/");
    Some(path.to_string())
}

/// Case-insensitive lookup of a request header value.
fn header_value<'a>(request: &'a str, name: &str) -> Option<&'a str> {
    request.lines().skip(1).find_map(|line| {
        let (key, value) = line.split_once(':')?;
        if key.trim().eq_ignore_ascii_case(name) {
            Some(value.trim())
        } else {
            None
        }
    })
}

fn accepts_gzip(request: &str) -> bool {
    header_value(request, "accept-encoding")
        .map(|v| {
            v.split(',').any(|enc| {
                let mut it = enc.split(';');
                let name = it.next().unwrap_or("").trim();
                let disabled = it.any(|p| {
                    let p = p.trim().replace(' ', "");
                    p == "q=0" || p == "q=0.0" || p == "q=0.00" || p == "q=0.000"
                });
                (name.eq_ignore_ascii_case("gzip") || name == "*") && !disabled
            })
        })
        .unwrap_or(false)
}

fn get_mime_type_from_path(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "webmanifest" => "application/manifest+json; charset=utf-8",
        "wasm" => "application/wasm",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// An embedded file, plus whether its stored bytes are gzip-compressed.
fn find_file(path: &str) -> Option<(&'static File<'static>, bool)> {
    let path = path.trim_start_matches('/');
    let path = if path.is_empty() || path.ends_with('/') {
        format!("{}index.html", path)
    } else {
        path.to_string()
    };
    if path.split('/').any(|seg| seg == ".." || seg == "." || seg.is_empty()) {
        return None;
    }
    if let Some(file) = STATIC_DIR.get_file(format!("{}.gz", path)) {
        return Some((file, true));
    }
    STATIC_DIR.get_file(&path).map(|f| (f, false))
}

/// Facts about this server exposed to the page at `/api/info`.
pub struct WebInfo {
    pub name: Option<String>,
    pub version: Option<String>,
    pub tcp_port: Option<u16>,
    pub tls_port: Option<u16>,
    pub mdns: bool,
    pub client_addr: Option<SocketAddr>,
}

impl WebInfo {
    pub fn new(config: &ServerConfig, client_addr: Option<SocketAddr>) -> Self {
        let tls = config.cert_path.is_some() && config.key_path.is_some();
        WebInfo {
            name: config.server_name.clone(),
            version: config.version.clone(),
            tcp_port: config.tcp_addresses.first().map(|a| a.port()),
            tls_port: if tls {
                config.tls_addresses.first().map(|a| a.port())
            } else {
                None
            },
            mdns: config.enable_mdns,
            client_addr,
        }
    }

    fn to_json(&self) -> String {
        serde_json::json!({
            "name": self.name,
            "version": self.version,
            "tcpPort": self.tcp_port,
            "tlsPort": self.tls_port,
            "mdnsHost": if self.mdns { Some("nettest.local") } else { None },
            "clientIp": self.client_addr.map(|a| normalize_ip(a).to_string()),
        })
        .to_string()
    }
}

/// Show IPv4-mapped IPv6 addresses (`::ffff:a.b.c.d`) as plain IPv4.
fn normalize_ip(addr: SocketAddr) -> std::net::IpAddr {
    match addr.ip() {
        std::net::IpAddr::V6(v6) => v6
            .to_ipv4_mapped()
            .map(std::net::IpAddr::V4)
            .unwrap_or(std::net::IpAddr::V6(v6)),
        ip => ip,
    }
}

/// Complete HTTP response (status line, headers and body) for a static
/// request. Always `Connection: close`.
pub fn build_response(request: &str, info: &WebInfo) -> (u16, Vec<u8>) {
    let is_head = request.starts_with("HEAD ");
    let path = match parse_http_path(request) {
        Some(p) => p,
        None => return simple_response(400, "Bad Request", is_head),
    };

    if path == "/api/info" {
        let body = info.to_json().into_bytes();
        let headers = vec![
            ("Content-Type", "application/json; charset=utf-8".to_string()),
            ("Cache-Control", "no-store".to_string()),
        ];
        return (200, assemble(200, "OK", &headers, &body, is_head));
    }

    let (file, gzipped) = match find_file(&path) {
        Some(found) => found,
        None => return simple_response(404, "Not Found", is_head),
    };
    let stored_path = file.path().to_str().unwrap_or("");
    let logical_path = stored_path.strip_suffix(".gz").unwrap_or(stored_path);
    let etag = ETAGS.get(stored_path).cloned().unwrap_or_default();

    let mut headers: Vec<(&str, String)> = vec![
        ("Content-Type", get_mime_type_from_path(logical_path).to_string()),
        // Small files on a LAN: always revalidate, the ETag makes it cheap.
        ("Cache-Control", "no-cache".to_string()),
        ("ETag", etag.clone()),
    ];
    if gzipped {
        headers.push(("Vary", "Accept-Encoding".to_string()));
    }
    if logical_path.ends_with(".html") {
        headers.push(("Content-Security-Policy", CSP.to_string()));
    }

    if let Some(inm) = header_value(request, "if-none-match") {
        if !etag.is_empty() && inm.split(',').any(|t| t.trim().trim_start_matches("W/") == etag) {
            return (304, assemble(304, "Not Modified", &headers, &[], true));
        }
    }

    let body: Vec<u8> = if gzipped && !accepts_gzip(request) {
        let mut out = Vec::new();
        if flate2::read::GzDecoder::new(file.contents())
            .read_to_end(&mut out)
            .is_err()
        {
            return simple_response(500, "Internal Server Error", is_head);
        }
        out
    } else {
        if gzipped {
            headers.push(("Content-Encoding", "gzip".to_string()));
        }
        file.contents().to_vec()
    };

    (200, assemble(200, "OK", &headers, &body, is_head))
}

fn simple_response(status: u16, reason: &str, is_head: bool) -> (u16, Vec<u8>) {
    let body = format!("{} {}\n", status, reason);
    let headers = vec![("Content-Type", "text/plain; charset=utf-8".to_string())];
    (status, assemble(status, reason, &headers, body.as_bytes(), is_head))
}

fn assemble(
    status: u16,
    reason: &str,
    headers: &[(&str, String)],
    body: &[u8],
    omit_body: bool,
) -> Vec<u8> {
    let mut head = format!("HTTP/1.1 {} {}\r\n", status, reason);
    for (k, v) in headers {
        head.push_str(k);
        head.push_str(": ");
        head.push_str(v);
        head.push_str("\r\n");
    }
    if status != 304 {
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    head.push_str("X-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n");
    let mut out = head.into_bytes();
    if !omit_body {
        out.extend_from_slice(body);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> WebInfo {
        WebInfo {
            name: Some("lab".into()),
            version: Some("2.1.3".into()),
            tcp_port: Some(5005),
            tls_port: None,
            mdns: true,
            client_addr: Some("[::ffff:192.168.1.7]:51000".parse().unwrap()),
        }
    }

    fn split(resp: &[u8]) -> (String, &[u8]) {
        let pos = resp.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
        (String::from_utf8_lossy(&resp[..pos]).to_string(), &resp[pos..])
    }

    #[test]
    fn upgrade_requests_are_not_static() {
        // Native client sends upper-case "RMBT".
        assert!(!is_static_file_request(
            "GET /rmbt HTTP/1.1 \r\nConnection: Upgrade\r\nUpgrade: RMBT\r\n\r\n"
        ));
        assert!(!is_static_file_request(
            "GET / HTTP/1.1\r\nHost: x\r\nupgrade: WebSocket\r\nConnection: Upgrade\r\n\r\n"
        ));
        assert!(is_static_file_request("GET / HTTP/1.1\r\nHost: x\r\n\r\n"));
        assert!(is_static_file_request("HEAD /app.js HTTP/1.1\r\nHost: x\r\n\r\n"));
        assert!(!is_static_file_request("POST / HTTP/1.1\r\nHost: x\r\n\r\n"));
    }

    #[test]
    fn path_parsing_strips_query() {
        assert_eq!(
            parse_http_path("GET /?autostart=0 HTTP/1.1\r\n\r\n").as_deref(),
            Some("/")
        );
        assert!(find_file("/../Cargo.toml").is_none());
    }

    #[test]
    fn serves_index_compressed_with_csp() {
        let (status, resp) = build_response(
            "GET / HTTP/1.1\r\nHost: x\r\nAccept-Encoding: gzip, deflate\r\n\r\n",
            &info(),
        );
        assert_eq!(status, 200);
        let (head, body) = split(&resp);
        assert!(head.contains("Content-Type: text/html"));
        assert!(head.contains("Content-Security-Policy"));
        assert!(head.contains("Connection: close"));
        if head.contains("Content-Encoding: gzip") {
            assert_eq!(&body[..2], &[0x1f, 0x8b]);
        }
    }

    #[test]
    fn decompresses_for_clients_without_gzip() {
        let (_, resp) = build_response("GET / HTTP/1.1\r\nHost: x\r\n\r\n", &info());
        let (head, body) = split(&resp);
        assert!(!head.contains("Content-Encoding"));
        assert!(String::from_utf8_lossy(body).to_lowercase().contains("<!doctype html"));
    }

    #[test]
    fn etag_revalidation_and_head() {
        let (_, resp) = build_response("GET / HTTP/1.1\r\n\r\n", &info());
        let (head, _) = split(&resp);
        let etag = head
            .lines()
            .find_map(|l| l.strip_prefix("ETag: "))
            .unwrap()
            .to_string();
        let (status, resp) = build_response(
            &format!("GET / HTTP/1.1\r\nIf-None-Match: {}\r\n\r\n", etag),
            &info(),
        );
        assert_eq!(status, 304);
        assert!(split(&resp).1.is_empty());

        let (status, resp) = build_response("HEAD / HTTP/1.1\r\n\r\n", &info());
        assert_eq!(status, 200);
        assert!(split(&resp).1.is_empty());
    }

    #[test]
    fn info_and_not_found() {
        let (status, resp) = build_response("GET /api/info HTTP/1.1\r\n\r\n", &info());
        assert_eq!(status, 200);
        let v: serde_json::Value = serde_json::from_slice(split(&resp).1).unwrap();
        assert_eq!(v["clientIp"], "192.168.1.7");
        assert_eq!(v["tcpPort"], 5005);
        assert_eq!(v["mdnsHost"], "nettest.local");

        let (status, _) = build_response("GET /nope.js HTTP/1.1\r\n\r\n", &info());
        assert_eq!(status, 404);
    }

    #[test]
    fn mime_types() {
        assert_eq!(get_mime_type_from_path("a/nettest_bg.wasm"), "application/wasm");
        assert_eq!(get_mime_type_from_path("x.SVG"), "image/svg+xml");
        assert!(accepts_gzip("GET / HTTP/1.1\r\naccept-encoding: br, gzip\r\n\r\n"));
        assert!(!accepts_gzip("GET / HTTP/1.1\r\nAccept-Encoding: gzip;q=0\r\n\r\n"));
    }
}
