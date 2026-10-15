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
use crate::mioserver::topology::{self, is_local_ip, NetView};
use crate::mioserver::upstream::{RunKind, StartError, Upstream};
use std::net::IpAddr;
use std::sync::Arc;

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

const WEB_METHODS: [&str; 4] = ["GET", "HEAD", "POST", "OPTIONS"];

/// True for a plain HTTP request that does not ask for a protocol upgrade,
/// i.e. a browser loading the page or calling `/api/*` rather than a client
/// starting a test (those always send `Upgrade:`).
pub fn is_static_file_request(request: &str) -> bool {
    let method = request.split_whitespace().next().unwrap_or("");
    if !WEB_METHODS.contains(&method) {
        return false;
    }
    header_value(request, "upgrade").is_none()
}

fn method_of(request: &str) -> &str {
    request.split_whitespace().next().unwrap_or("")
}

/// Request target of the first line, without query string or fragment.
pub fn parse_http_path(request: &str) -> Option<String> {
    let first_line = request.lines().next()?;
    let mut parts = first_line.split_whitespace();
    let method = parts.next()?;
    if !WEB_METHODS.contains(&method) {
        return None;
    }
    let target = parts.next()?;
    let path = target.split(['?', '#']).next().unwrap_or("/");
    Some(path.to_string())
}

/// Value of `name` in the query string of the request line.
fn query_param<'a>(request: &'a str, name: &str) -> Option<&'a str> {
    let target = request.lines().next()?.split_whitespace().nth(1)?;
    let query = target.split('#').next()?.split_once('?')?.1;
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        (k == name).then_some(v)
    })
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
    /// Set when the page may trigger the server ↔ internet measurement.
    pub upstream: Option<Arc<Upstream>>,
    /// How the client reaches this server (`topology::path_json`).
    pub path: serde_json::Value,
    /// `-demo`: the page skips the path checks.
    pub demo: bool,
}

impl WebInfo {
    pub fn new(config: &ServerConfig, client_addr: Option<SocketAddr>) -> Self {
        let tls = config.cert_path.is_some() && config.key_path.is_some();
        WebInfo {
            upstream: config.upstream.clone(),
            path: serde_json::Value::Null,
            demo: config.demo,
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

    /// Classify the client's network path (reads interfaces and routes, so
    /// only done for `/api/info`).
    pub fn with_path(mut self, config: &ServerConfig) -> Self {
        let lan_url = crate::mioserver::banner::web_ui_urls(config).into_iter().next();
        self.path = topology::path_json(&NetView::current(), self.client_addr.map(|a| a.ip()), lan_url);
        self
    }

    fn to_json(&self) -> String {
        serde_json::json!({
            "name": self.name,
            "version": self.version,
            "tcpPort": self.tcp_port,
            "tlsPort": self.tls_port,
            "mdnsHost": if self.mdns { Some("nettest.local") } else { None },
            "clientIp": self.client_addr.map(|a| normalize_ip(a).to_string()),
            "upstream": self.upstream.is_some(),
            "path": self.path,
            "demo": self.demo,
        })
        .to_string()
    }
}

// ---------------------------------------------------------------------------
// /api/upstream/*: the server ↔ internet leg
// ---------------------------------------------------------------------------

/// Header the page sends with every upstream call. A custom header cannot be
/// set by a plain cross-site form or image, and on `fetch` it forces a CORS
/// preflight, which this server never approves.
const API_HEADER: &str = "x-nettest";

/// `GET /api/ip`: the caller's address as this server sees it. Readable from
/// any origin (nothing else in the answer), so the page on a home server can
/// ask a public server which public address its traffic leaves from. Served
/// even with the web UI off, for the public measurement servers.
pub fn is_ip_echo_request(request: &str) -> bool {
    matches!(method_of(request), "GET" | "HEAD" | "OPTIONS")
        && parse_http_path(request).as_deref() == Some("/api/ip")
}

fn ip_echo_response(request: &str, client: Option<SocketAddr>) -> (u16, Vec<u8>) {
    let is_head = method_of(request) == "HEAD";
    let headers = vec![
        ("Content-Type", "application/json; charset=utf-8".to_string()),
        ("Cache-Control", "no-store".to_string()),
        ("Access-Control-Allow-Origin", "*".to_string()),
    ];
    if method_of(request) == "OPTIONS" {
        return (204, assemble(204, "No Content", &headers, &[], true));
    }
    let body = serde_json::json!({ "ip": client.map(|a| normalize_ip(a).to_string()) });
    (200, assemble(200, "OK", &headers, body.to_string().as_bytes(), is_head))
}

/// A `Host` the page can legitimately have been loaded from: an address
/// literal or a local name. Rejects DNS-rebinding names like `evil.example`.
fn is_local_host(host: &str) -> bool {
    let host = host.trim();
    let name = if let Some(rest) = host.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        host.rsplit_once(':').map(|(h, p)| if p.chars().all(|c| c.is_ascii_digit()) { h } else { host }).unwrap_or(host)
    };
    if name.is_empty() {
        return false;
    }
    if let Ok(ip) = name.parse::<IpAddr>() {
        return is_local_ip(ip);
    }
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    !name.contains('.')
        || [".local", ".lan", ".home", ".home.arpa", ".internal", ".localdomain"]
            .iter()
            .any(|s| name.ends_with(s))
}

/// The checks every `/api/upstream/*` call must pass, as `(status, reason)`.
fn upstream_guard(request: &str, client: Option<SocketAddr>) -> Result<(), (u16, &'static str)> {
    if !client.is_some_and(|a| is_local_ip(a.ip())) {
        return Err((403, "only devices on the local network may use this"));
    }
    if !header_value(request, "host").is_some_and(is_local_host) {
        return Err((403, "unexpected Host"));
    }
    if header_value(request, API_HEADER) != Some("1") {
        return Err((403, "missing X-Nettest header"));
    }
    // Browsers send Origin on POST; it must be this very page.
    if let (Some(origin), Some(host)) = (header_value(request, "origin"), header_value(request, "host")) {
        let origin_host = origin.split_once("://").map(|(_, h)| h).unwrap_or("");
        if !origin_host.eq_ignore_ascii_case(host) {
            return Err((403, "cross-origin request"));
        }
    }
    Ok(())
}

fn json_response(status: u16, reason: &str, body: &serde_json::Value, is_head: bool) -> (u16, Vec<u8>) {
    let headers = vec![
        ("Content-Type", "application/json; charset=utf-8".to_string()),
        ("Cache-Control", "no-store".to_string()),
    ];
    (status, assemble(status, reason, &headers, body.to_string().as_bytes(), is_head))
}

fn error_json(status: u16, reason: &str, error: &str) -> (u16, Vec<u8>) {
    json_response(status, reason, &serde_json::json!({ "error": error }), false)
}

fn upstream_response(request: &str, path: &str, info: &WebInfo) -> (u16, Vec<u8>) {
    let method = method_of(request);
    let is_head = method == "HEAD";
    // Off: behave as if the endpoints did not exist.
    let Some(up) = &info.upstream else {
        return simple_response(404, "Not Found", is_head);
    };
    // No CORS preflight is ever approved.
    if method == "OPTIONS" {
        return error_json(403, "Forbidden", "cross-origin requests are not allowed");
    }
    if let Err((status, why)) = upstream_guard(request, info.client_addr) {
        return error_json(status, "Forbidden", why);
    }
    match (path, method) {
        ("/api/upstream/start", "POST") => {
            // `?kind=speed|quality|full`; older pages send nothing (full).
            let kind = match query_param(request, "kind") {
                None => RunKind::Full,
                Some(k) => match RunKind::parse(k) {
                    Some(kind) => kind,
                    None => return error_json(400, "Bad Request", "kind must be speed, quality or full"),
                },
            };
            upstream_start(up, kind)
        }
        ("/api/upstream/status", "GET" | "HEAD") => json_response(200, "OK", &up.status_json(), is_head),
        ("/api/upstream/last", "GET" | "HEAD") => json_response(200, "OK", &up.last_json(), is_head),
        ("/api/upstream/start" | "/api/upstream/status" | "/api/upstream/last", _) => {
            error_json(405, "Method Not Allowed", "method not allowed")
        }
        _ => simple_response(404, "Not Found", is_head),
    }
}

fn upstream_start(up: &Arc<Upstream>, kind: RunKind) -> (u16, Vec<u8>) {
    match up.start(kind) {
            Ok(id) => json_response(202, "Accepted", &serde_json::json!({ "id": id }), false),
            Err(StartError::Running) => error_json(409, "Conflict", "a measurement is already running"),
            Err(StartError::TooSoon(wait)) => {
                let secs = wait.as_secs_f64().ceil() as u64;
                let mut body = up.last_json();
                body["error"] = "too soon after the previous run".into();
                body["retryAfterS"] = secs.into();
                let (_, mut resp) = json_response(429, "Too Many Requests", &body, false);
                // Insert Retry-After right after the status line.
                let at = resp.windows(2).position(|w| w == b"\r\n").map(|p| p + 2).unwrap_or(0);
                resp.splice(at..at, format!("Retry-After: {}\r\n", secs).into_bytes());
                (429, resp)
            }
    }
}

/// Show IPv4-mapped IPv6 addresses (`::ffff:a.b.c.d`) as plain IPv4.
fn normalize_ip(addr: SocketAddr) -> IpAddr {
    topology::canonical(addr.ip())
}

/// Complete HTTP response (status line, headers and body) for a static
/// request. Always `Connection: close`.
pub fn build_response(request: &str, info: &WebInfo) -> (u16, Vec<u8>) {
    let is_head = request.starts_with("HEAD ");
    let path = match parse_http_path(request) {
        Some(p) => p,
        None => return simple_response(400, "Bad Request", is_head),
    };

    if path.starts_with("/api/upstream/") {
        return upstream_response(request, &path, info);
    }
    if is_ip_echo_request(request) {
        return ip_echo_response(request, info.client_addr);
    }
    let method = method_of(request);
    if method != "GET" && method != "HEAD" {
        return simple_response(405, "Method Not Allowed", is_head);
    }

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
            upstream: None,
            path: serde_json::json!({ "relation": "same-lan", "legsValid": true }),
            demo: false,
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
        assert!(!is_static_file_request("PUT / HTTP/1.1\r\nHost: x\r\n\r\n"));
        assert!(is_static_file_request("POST /api/upstream/start HTTP/1.1\r\nHost: x\r\n\r\n"));
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

    fn upstream_info(client: &str) -> WebInfo {
        use crate::mioserver::upstream::UpstreamSettings;
        WebInfo {
            upstream: Some(Arc::new(Upstream::new(UpstreamSettings {
                // Unreachable on purpose: a started run fails fast, offline.
                server: Some("127.0.0.1:9".into()),
                prefer_on_net: true,
                control_server: "http://127.0.0.1:9".into(),
                x_nettest_client: "nt".into(),
                min_interval: std::time::Duration::from_secs(60),
            }))),
            client_addr: Some(client.parse().unwrap()),
            ..info()
        }
    }

    const STATUS: &str = "GET /api/upstream/status HTTP/1.1\r\nHost: 192.168.1.2:5005\r\nX-Nettest: 1\r\n\r\n";

    #[test]
    fn upstream_off_is_404_and_info_flags() {
        for req in [STATUS, "POST /api/upstream/start HTTP/1.1\r\nHost: 192.168.1.2\r\nX-Nettest: 1\r\n\r\n"] {
            assert_eq!(build_response(req, &info()).0, 404);
        }
        let (_, resp) = build_response("GET /api/info HTTP/1.1\r\n\r\n", &info());
        let v: serde_json::Value = serde_json::from_slice(split(&resp).1).unwrap();
        assert_eq!(v["upstream"], false);
        let (_, resp) = build_response("GET /api/info HTTP/1.1\r\n\r\n", &upstream_info("192.168.1.7:1"));
        let v: serde_json::Value = serde_json::from_slice(split(&resp).1).unwrap();
        assert_eq!(v["upstream"], true);
    }

    #[test]
    fn upstream_guard_rejections() {
        let lan = upstream_info("192.168.1.7:40000");
        assert_eq!(build_response(STATUS, &lan).0, 200);
        // Public source address.
        assert_eq!(build_response(STATUS, &upstream_info("203.0.113.9:40000")).0, 403);
        // Missing header.
        let no_header = "GET /api/upstream/status HTTP/1.1\r\nHost: 192.168.1.2:5005\r\n\r\n";
        assert_eq!(build_response(no_header, &lan).0, 403);
        // DNS rebinding: foreign Host.
        let rebind = "GET /api/upstream/status HTTP/1.1\r\nHost: evil.example:5005\r\nX-Nettest: 1\r\n\r\n";
        assert_eq!(build_response(rebind, &lan).0, 403);
        // Cross-origin POST.
        let xo = "POST /api/upstream/start HTTP/1.1\r\nHost: 192.168.1.2:5005\r\nOrigin: http://evil.example\r\nX-Nettest: 1\r\n\r\n";
        assert_eq!(build_response(xo, &lan).0, 403);
        // Preflight is never approved.
        let pre = "OPTIONS /api/upstream/start HTTP/1.1\r\nHost: 192.168.1.2:5005\r\nOrigin: http://evil.example\r\n\r\n";
        let (status, resp) = build_response(pre, &lan);
        assert_eq!(status, 403);
        assert!(!split(&resp).0.to_ascii_lowercase().contains("access-control-allow"));
        // Wrong method.
        let get_start = "GET /api/upstream/start HTTP/1.1\r\nHost: 192.168.1.2:5005\r\nX-Nettest: 1\r\n\r\n";
        assert_eq!(build_response(get_start, &lan).0, 405);
        // POST elsewhere is not a thing.
        assert_eq!(build_response("POST / HTTP/1.1\r\nHost: x\r\n\r\n", &lan).0, 405);
    }

    #[test]
    fn ip_echo_is_cross_origin_readable() {
        let req = "GET /api/ip HTTP/1.1\r\nHost: x\r\nOrigin: http://192.168.0.2:5005\r\n\r\n";
        assert!(is_ip_echo_request(req));
        assert!(!is_ip_echo_request("GET /api/info HTTP/1.1\r\n\r\n"));
        let (status, resp) = build_response(req, &info());
        assert_eq!(status, 200);
        let (head, body) = split(&resp);
        assert!(head.contains("Access-Control-Allow-Origin: *"));
        let v: serde_json::Value = serde_json::from_slice(body).unwrap();
        assert_eq!(v, serde_json::json!({ "ip": "192.168.1.7" }));
        // /api/info stays same-origin only.
        let (_, resp) = build_response("GET /api/info HTTP/1.1\r\n\r\n", &info());
        assert!(!split(&resp).0.contains("Access-Control"));
        let v: serde_json::Value = serde_json::from_slice(split(&resp).1).unwrap();
        assert_eq!(v["path"]["relation"], "same-lan");
    }

    #[test]
    fn local_hosts() {
        for h in ["192.168.0.2:5005", "10.0.0.1", "[fe80::1]:5005", "[::1]:80", "localhost:5005", "nettest.local", "router.lan", "box", "speed.home.arpa:5005"] {
            assert!(is_local_host(h), "{h}");
        }
        for h in ["evil.example", "8.8.8.8:5005", "[2001:db8::1]:80", "", "nettest.local.evil.com"] {
            assert!(!is_local_host(h), "{h}");
        }
    }

    #[test]
    fn upstream_start_conflict_and_rate_limit() {
        let lan = upstream_info("[fd00::7]:40000");
        let start = "POST /api/upstream/start HTTP/1.1\r\nHost: nettest.local:5005\r\nOrigin: http://nettest.local:5005\r\nX-Nettest: 1\r\nContent-Length: 0\r\n\r\n";
        assert_eq!(build_response(start, &lan).0, 202);
        // Either still running (409) or already failed against port 9 (429).
        let second = build_response(start, &lan).0;
        assert!(second == 409 || second == 429, "{second}");
        // Wait for the offline run to fail, then the pause applies.
        let up = lan.upstream.clone().unwrap();
        for _ in 0..100 {
            if up.status_json()["running"] == false {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let (status, resp) = build_response(start, &lan);
        assert_eq!(status, 429);
        let (head, body) = split(&resp);
        assert!(head.contains("Retry-After: "));
        let v: serde_json::Value = serde_json::from_slice(body).unwrap();
        assert!(v["last"]["error"].is_string());
    }
}
