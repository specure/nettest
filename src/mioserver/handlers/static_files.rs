use log::{debug, info};
use std::fs;
use std::io::{self};
use std::path::{Path, PathBuf};
use crate::stream::stream::Stream;

const STATIC_DIR_NAME: &str = "dist";

/// Get the path to the static files directory (next to the binary)
fn get_static_dir_path() -> io::Result<PathBuf> {
    // Get the path to the current executable
    let exe_path = std::env::current_exe()
        .map_err(|e| io::Error::new(io::ErrorKind::NotFound, format!("Failed to get executable path: {}", e)))?;
    
    // Get the directory containing the executable
    let exe_dir = exe_path.parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Executable has no parent directory"))?;
    
    // Build path to static files directory
    let static_dir = exe_dir.join(STATIC_DIR_NAME);
    
    debug!("Static files directory: {:?}", static_dir);
    Ok(static_dir)
}

pub fn serve_static_file(path: &str, stream: &mut Stream) -> io::Result<bool> {
    // Normalize path - remove leading slash and handle root
    let file_path = if path == "/" || path.is_empty() {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };

    // Get static directory path (next to binary)
    let static_dir = get_static_dir_path()?;
    
    // Build full path to static file
    let file_full_path = static_dir.join(file_path);
    
    // Security: prevent path traversal
    let canonical_static = static_dir.canonicalize()
        .map_err(|_| io::Error::new(io::ErrorKind::NotFound, "Static directory not found"))?;
    let canonical_file = file_full_path.canonicalize()
        .map_err(|_| io::Error::new(io::ErrorKind::NotFound, format!("File not found: {}", file_path)))?;
    
    if !canonical_file.starts_with(&canonical_static) {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Path traversal detected"));
    }

    debug!("Serving static file: {:?}", canonical_file);

    // Read file
    let content = fs::read(&canonical_file)?;
    
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
    stream.write(&content)?;
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
    } else if path_lower.ends_with(".png") {
        "image/png"
    } else if path_lower.ends_with(".jpg") || path_lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if path_lower.ends_with(".gif") {
        "image/gif"
    } else if path_lower.ends_with(".svg") {
        "image/svg+xml"
    } else if path_lower.ends_with(".json") {
        "application/json"
    } else if path_lower.ends_with(".woff") {
        "font/woff"
    } else if path_lower.ends_with(".woff2") {
        "font/woff2"
    } else if path_lower.ends_with(".ttf") {
        "font/ttf"
    } else if path_lower.ends_with(".eot") {
        "application/vnd.ms-fontobject"
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
