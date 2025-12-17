use log::{debug, info};
use std::io::{self};
use include_dir::{include_dir, Dir};
use crate::stream::stream::Stream;

// Embed the dist directory at compile time
static STATIC_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/dist");

pub fn serve_static_file(path: &str, stream: &mut Stream) -> io::Result<bool> {
    // Normalize path - remove leading slash and handle root
    let file_path = if path == "/" || path.is_empty() {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };

    debug!("Looking for static file: {}", file_path);

    // Find file in embedded static directory
    let file = STATIC_DIR.get_file(file_path)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("File not found: {}", file_path)))?;

    // Get file contents
    let content = file.contents();
    
    // Determine MIME type from file path
    let mime_type = get_mime_type_from_path(file_path);
    
    // Build HTTP response
    let response = format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: {}\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n",
        mime_type,
        content.len()
    );

    // Send response header
    stream.write(response.as_bytes())?;
    // Send file content
    stream.write(content)?;
    stream.flush()?;

    info!("Served static file: {} ({} bytes)", file_path, content.len());
    Ok(true)
}

fn get_mime_type_from_path(path: &str) -> &'static str {
    let path_lower = path.to_lowercase();
    if path_lower.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if path_lower.ends_with(".js") {
        "application/javascript; charset=utf-8"
    } else if path_lower.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if path_lower.ends_with(".ico") {
        "image/x-icon"
    } else {
        "application/octet-stream"
    }
}

pub fn is_static_file_request(request: &str) -> bool {
    // Check if it's a GET request that's not an upgrade request
    if !request.starts_with("GET ") {
        return false;
    }

    // Check if it's not a WebSocket or RMBT upgrade request
    let is_websocket = request.contains("Upgrade: websocket") || request.contains("upgrade: websocket");
    let is_rmbt = request.contains("Upgrade: rmbt") || request.contains("upgrade: rmbt");
    
    !is_websocket && !is_rmbt
}

pub fn parse_http_path(request: &str) -> Option<String> {
    // Parse GET /path HTTP/1.1
    let lines: Vec<&str> = request.lines().collect();
    if let Some(first_line) = lines.first() {
        let parts: Vec<&str> = first_line.split_whitespace().collect();
        if parts.len() >= 2 && parts[0] == "GET" {
            return Some(parts[1].to_string());
        }
    }
    None
}
