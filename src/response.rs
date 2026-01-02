use std::fs;
use uuid::Uuid;

use crate::request::HttpRequest;
use crate::{
    config::{Route, ServerConfig},
    request::HttpRequestBuilder,
    server,
    utils::HttpHeaders,
};

pub struct HttpResponseBuilder {
    status_code: u16,
    status_text: String,
    headers: HttpHeaders,
    body: Vec<u8>,
}

impl HttpResponseBuilder {
    pub fn new(status_code: u16, status_text: &str) -> Self {
        Self {
            status_code,
            status_text: status_text.to_string(),
            headers: HttpHeaders::new(),
            body: Vec::new(),
        }
    }

    pub fn header(mut self, key: &str, value: &str) -> Self {
        self.headers.insert(key, value);
        self
    }

    pub fn body(mut self, body: Vec<u8>) -> Self {
        self.body = body;
        self
    }

    pub fn build(mut self) -> Vec<u8> {
        // Auto-add Content-Length if not present
        self.headers
            .insert("Content-Length", &self.body.len().to_string());

        let mut response = format!("HTTP/1.1 {} {}\r\n", self.status_code, self.status_text);

        for (key, value) in self.headers.iter() {
            response.push_str(&format!("{}: {}\r\n", key, value));
        }

        response.push_str("\r\n");

        let mut bytes = response.into_bytes();
        bytes.extend_from_slice(&self.body);
        bytes
    }

    // === Convenience methods ===

    pub fn ok() -> Self {
        Self::new(200, "OK")
    }
    pub fn created() -> Self {
        Self::new(201, "Created")
    }

    pub fn not_found() -> Self {
        Self::new(404, "Not Found")
    }

    pub fn method_not_allowed() -> Self {
        Self::new(405, "Method Not Allowed")
    }

    pub fn no_content() -> Self {
        Self::new(204, "No Content")
    }

    pub fn internal_error() -> Self {
        Self::new(500, "Internal Server Error")
    }
    pub fn bad_request() -> Self {
        Self::new(400, "Bad Request")
    }

    pub fn unsupported_media_type() -> Self {
        Self::new(415, "Unsupported Media Type")
    }

    // === File serving methods ===
    /// Serve a directory listing as HTML
    pub fn serve_directory_listing(dir_path: &str, route_path: &str) -> Vec<u8> {
        let mut listing = String::from("<html><body><h1>Directory Listing</h1><ul>");

        if let Ok(entries) = fs::read_dir(dir_path) {
            for entry in entries {
                println!("Reading directory entry for listing");
                if let Ok(entry) = entry {
                    let file_name = entry.file_name();
                    let file_name_str = file_name.to_string_lossy();
                    listing.push_str(&format!(
                        "<li><a href=\"{}\\{}\">{}</a></li>",
                        route_path, file_name_str, file_name_str
                    ));
                }
            }
        }

        listing.push_str("</ul></body></html>");
        Self::ok().body(listing.into_bytes().to_vec()).build()
    }

    /// Serve a file with automatic content-type detection
    pub fn serve_file(path: &str) -> Result<Vec<u8>, std::io::Error> {
        let content = fs::read(path)?;
        let content_type = detect_content_type(path);

        Ok(Self::ok()
            .header("Content-Type", content_type)
            .body(content)
            .build())
    }

    /// Serve a custom error page or fall back to minimal response
    pub fn serve_error_page(error_page_path: &str, status_code: u16, status_text: &str) -> Vec<u8> {
        match fs::read(error_page_path) {
            Ok(content) => {
                println!(
                    "Serving custom {} error page from: {}",
                    status_code, error_page_path
                );
                Self::new(status_code, status_text)
                    .header("Content-Type", "text/html")
                    .body(content)
                    .build()
            }
            Err(_) => {
                println!(
                    "Error page '{}' not found, sending minimal {} response",
                    error_page_path, status_code
                );
                Self::new(status_code, status_text).build()
            }
        }
    }

    /// Try to serve a file, or serve 404 error page on failure
    pub fn serve_file_or_404(file_path: &str, error_page_path: &str) -> Vec<u8> {
        println!("Attempting to serve file: {}", file_path);

        match Self::serve_file(file_path) {
            Ok(response) => {
                println!("File found, serving 200 OK");
                response
            }
            Err(_) => {
                println!("File not found: {}, serving 404 page", file_path);
                Self::serve_error_page(error_page_path, 404, "Not Found")
            }
        }
    }
}

// Helper function to detect content type from file extension
fn detect_content_type(path: &str) -> &'static str {
    if path.ends_with(".html") || path.ends_with(".htm") {
        "text/html"
    } else if path.ends_with(".css") {
        "text/css"
    } else if path.ends_with(".js") {
        "application/javascript"
    } else if path.ends_with(".json") {
        "application/json"
    } else if path.ends_with(".png") {
        "image/png"
    } else if path.ends_with(".jpg") || path.ends_with(".jpeg") {
        "image/jpeg"
    } else if path.ends_with(".gif") {
        "image/gif"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else if path.ends_with(".txt") {
        "text/plain"
    } else {
        "application/octet-stream"
    }
}

// === Handler functions for different HTTP methods ===

pub fn handle_get(request_path: &str, server: &ServerConfig, request: &HttpRequest) -> Vec<u8> {
    if let Some(route) = server.routes.iter().find(|r| r.path == request.path) {
        // Directory listing allowed?
        if route.list_directory == Some(true) {
            return HttpResponseBuilder::serve_directory_listing(&request_path, &route.path);
        }

        // Default file exists? Serve it
        if let Some(default_file) = &route.default_file {
            let server_root = &server.root;
            let root = &route.root;
            let full_path = format!("{}/{}/{}", server_root, root, default_file);
            return HttpResponseBuilder::serve_file_or_404(
                &full_path,
                &get_error_page_path(server, 404),
            );
        }
    }

    // If no route or no listing/default file, try to serve the requested file directly
    let error_page_path = get_error_page_path(server, 404);
    HttpResponseBuilder::serve_file_or_404(request_path, &error_page_path)
}

pub fn handle_delete(file_path: &str, error_page_path: &str) -> Vec<u8> {
    match fs::remove_file(file_path) {
        Ok(_) => {
            println!("DELETE: Successfully deleted {}", file_path);
            HttpResponseBuilder::no_content().build()
        }
        Err(_) => {
            println!("DELETE: File not found {}", file_path);
            HttpResponseBuilder::serve_error_page(error_page_path, 404, "Not Found")
        }
    }
}

pub fn handle_method_not_allowed(allowed_methods: &[String], server: &ServerConfig) -> Vec<u8> {
    let allow_header = allowed_methods.join(", ");

    // Get the path of the 405 error page, fallback to default
    let path = server
        .error_pages
        .iter()
        .find(|page| page.code == 405)
        .map(|page| page.path.as_str())
        .unwrap_or("./errors_pages/405.html");

    // Read the file content
    match fs::read(path) {
        Ok(content) => HttpResponseBuilder::method_not_allowed()
            .header("Allow", &allow_header)
            .header("Content-Type", "text/html")
            .body(content)
            .build(),
        Err(_) => HttpResponseBuilder::method_not_allowed()
            .header("Allow", &allow_header)
            .header("Content-Type", "text/plain")
            .body(b"Method Not Allowed".to_vec())
            .build(),
    }
}

fn get_error_page_path(server: &ServerConfig, status_code: u16) -> String {
    server
        .error_pages
        .iter()
        .find(|ep| ep.code == status_code)
        .map(|ep| ep.path.clone())
        .unwrap_or_else(|| format!("./error_pages/{}.html", status_code))
}

pub fn handle_post(file_path: &str, request: &HttpRequest) -> Vec<u8> {
    let body = match &request.body {
        Some(b) => b,
        None => {
            return HttpResponseBuilder::bad_request()
                .body(b"Empty body".to_vec())
                .build();
        }
    };

    if let Ok(s) = std::str::from_utf8(body) {
        println!("body as string: {}", s);
    } else {
        println!("body is binary, cannot print as string");
    }
    let content_type = match request.headers.get("content-type") {
        Some(v) => v,
        None => {
            return HttpResponseBuilder::bad_request()
                .body(b"Missing Content-Type".to_vec())
                .build();
        }
    };

    if content_type.starts_with("application/")
        || content_type.starts_with("image/")
        || content_type.starts_with("audio/")
        || content_type.starts_with("video/")
        || content_type.starts_with("font/") || content_type .starts_with("text/")
    {
        // get file extension from content type
        let b = content_type.split('/').nth(1).unwrap_or("dat");
        // For direct uploads, extract filename from the request path

        let filename: String = {
            let last_segment = request.path.split('/').last().unwrap_or("");

            if last_segment.contains('.') && !last_segment.is_empty() {
                last_segment.to_string()
            } else {
                format!("upload_{}.{}", Uuid::new_v4(), b)
            }
        };

        println!("Direct upload filename: {}  and  save path is  {}", filename, file_path);

        let save_path = if file_path.ends_with('/') {
            format!("{}{}", file_path, filename)
        } else {
            format!("{}", file_path)
        };

        println!("Saving non-multipart file to: {}", save_path);
        return write_file(&save_path, body);
    }

    if content_type.starts_with("multipart/form-data") {
        let boundary = match extract_boundary(content_type) {
            Some(b) => b,
            None => {
                return HttpResponseBuilder::bad_request()
                    .body(b"Missing multipart boundary".to_vec())
                    .build();
            }
        };

        println!("Extracted boundary: {}", boundary);

        let files = extract_multipart_files(body, &boundary);

        if files.is_empty() {
            return HttpResponseBuilder::bad_request()
                .body(b"Invalid multipart body or no files found".to_vec())
                .build();
        }

        // Write each file with its extracted filename
        let mut saved_files = Vec::new();
        for (filename, file_bytes) in files.iter() {
            // Combine the directory from file_path with the extracted filename
            let save_path = if file_path.ends_with('/') {
                format!("{}{}", file_path, filename)
            } else {
                format!("{}/{}", file_path, filename)
            };

            let response = write_file(&save_path, file_bytes);
            // Check if write failed
            if response.starts_with(b"HTTP/1.1 500") || response.starts_with(b"HTTP/1.1 4") {
                return response;
            }
            saved_files.push(filename.clone());
        }

        HttpResponseBuilder::created()
            .body(
                format!(
                    "Successfully uploaded {} file(s): {}",
                    saved_files.len(),
                    saved_files.join(", ")
                )
                .into_bytes(),
            )
            .build()
    } else {
        println!("Unsupported Content-Type: {}", content_type);
        HttpResponseBuilder::unsupported_media_type()
            .body(b"Unsupported Content-Type".to_vec())
            .build()
    }
}

fn write_file(path: &str, data: &[u8]) -> Vec<u8> {
    if let Ok(s) = std::str::from_utf8(data) {
        println!("body as string: {}", s);
    } else {
        println!("body is binary, cannot print as string");
    }

    println!("Writing file to: {}", path);
    match fs::write(path, data) {
        Ok(_) => HttpResponseBuilder::ok()
            .header("Content-Type", "text/plain")
            .body(b"Upload successful".to_vec())
            .build(),
        Err(e) => HttpResponseBuilder::internal_error()
            .body(e.to_string().into_bytes())
            .build(),
    }
}
fn extract_boundary(content_type: &str) -> Option<String> {
    content_type
        .split(';')
        .find(|s| s.trim().starts_with("boundary="))
        .map(|s| s.trim().trim_start_matches("boundary=").to_string())
}

fn extract_multipart_files<'a>(body: &'a [u8], boundary: &'a str) -> Vec<(String, &'a [u8])> {
    let boundary = format!("--{}", boundary);
    let body_str = match std::str::from_utf8(body) {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    let parts: Vec<&str> = body_str.split(&boundary).collect();
    let mut files = Vec::new();

    for part in parts.iter() {
        // Skip parts that don't contain Content-Disposition (not file parts)
        if !part.contains("Content-Disposition") {
            continue;
        }

        // Extract filename from Content-Disposition header
        let filename = extract_filename_from_disposition(part);
        if filename.is_none() {
            continue;
        }
        let filename = filename.unwrap();

        // Find the end of headers (blank line separates headers from data)
        let header_end = match part.find("\r\n\r\n") {
            Some(pos) => pos,
            None => continue,
        };

        let data_start = header_end + 4;
        let data = &part[data_start..];

        // Clean up trailing boundary markers and whitespace
        let data = data.trim_end_matches("\r\n").trim_end_matches("--");

        // Find the actual byte position in the original body
        let start = match body_str.find(data) {
            Some(pos) => pos,
            None => continue,
        };

        println!("Extracted file '{}' of length: {}", filename, data.len());
        files.push((filename, &body[start..start + data.len()]));
    }

    files
}

fn extract_filename_from_disposition(part: &str) -> Option<String> {
    // Find the Content-Disposition line
    let disposition_line = part
        .lines()
        .find(|line| line.contains("Content-Disposition"))?;

    // Look for filename="..." or filename*=...
    if let Some(start) = disposition_line.find("filename=\"") {
        let start = start + 10; // length of 'filename="'
        let end = disposition_line[start..].find('"')?;
        return Some(disposition_line[start..start + end].to_string());
    }

    // Fallback: look for filename= without quotes
    if let Some(start) = disposition_line.find("filename=") {
        let start = start + 9; // length of 'filename='
        let end = disposition_line[start..]
            .find(|c: char| c == ';' || c == '\r' || c == '\n')
            .unwrap_or(disposition_line[start..].len());
        return Some(disposition_line[start..start + end].trim().to_string());
    }

    None
}
