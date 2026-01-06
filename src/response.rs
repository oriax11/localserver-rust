use std::fs;

use crate::{
    config::ServerConfig,
    utils::{HttpHeaders, cookie::{self, Cookie}},
};

pub struct HttpResponseBuilder {
    status_code: u16,
    status_text: String,
    headers: HttpHeaders,
    cookies: Vec<Cookie>, // <-- new

    body: Vec<u8>,
}

impl HttpResponseBuilder {
    pub fn new(status_code: u16, status_text: &str) -> Self {
        Self {
            status_code,
            status_text: status_text.to_string(),
            headers: HttpHeaders::new(),
            body: Vec::new(),
            cookies: Vec::new(),
        }
    }

    pub fn header(mut self, key: &str, value: &str) -> Self {
        self.headers.insert(key, value);
        self
    }

    pub fn headers(mut self, headers: HttpHeaders) -> Self {
        self.headers = headers;
        self
    }
    // Add a cookie
    pub fn cookie(mut self, cookie: &Cookie) -> Self {
        self.cookies.push(cookie.clone());
        self
    }

    // Add multiple cookies if you want
    pub fn cookies(mut self, cookies: Vec<Cookie>) -> Self {
        self.cookies.extend(cookies);
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
        // Inject all cookies as headers
        for cookie in self.cookies.iter() {
            let (key, value) = cookie.to_header_pair();
            self.headers.insert(&key, &value);
        }

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

    pub fn redirect(location: &String) -> Self {
        Self::new(302, "Found").header("Location", location)
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
    pub fn serve_directory_listing(
        server_root: &str,
        route_root: &str,
        route_path: &str,
        cookie: &Cookie,
    ) -> Vec<u8> {
        let mut listing = String::from("<html><body><h1>Directory Listing</h1><ul>");

        let dir_path = format!("{}/{}", server_root, route_root);
        if let Ok(entries) = fs::read_dir(dir_path) {
            for entry in entries {
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

        let (key, value) = cookie.to_header_pair();

        Self::ok()
            .body(listing.into_bytes().to_vec())
            .cookie(cookie)
            .build()
    }

    /// Serve a file with automatic content-type detection
    pub fn serve_file(path: &str, cookie :&Cookie) -> Result<Vec<u8>, std::io::Error> {
        let content = fs::read(path)?;
        let content_type = detect_content_type(path);

        Ok(Self::ok()
            .header("Content-Type", content_type)
            .body(content)
            .cookie(cookie)
            .build())
    }

    /// Serve a custom error page or fall back to minimal response
    pub fn serve_error_page(error_page_path: &str, status_code: u16, status_text: &str , cookie :&Cookie   ) -> Vec<u8> {
        match fs::read(error_page_path) {
            Ok(content) => {
                println!(
                    "Serving custom {} error page from: {}",
                    status_code, error_page_path
                );
                Self::new(status_code, status_text)
                    .header("Content-Type", "text/html")
                    .body(content)
                    .cookie(cookie)
                    .build()
            }
            Err(_) => {
                println!(
                    "Error page '{}' not found, sending minimal {} response",
                    error_page_path, status_code
                );
                Self::new(status_code, status_text).cookie(cookie).build()
            }
        }
    }

    /// Try to serve a file, or serve 404 error page on failure
    pub fn serve_file_or_404(file_path: &str, error_page_path: &str, cookie: &Cookie) -> Vec<u8> {
        println!("Attempting to serve file: {}", file_path);

        match Self::serve_file(file_path , cookie) {
            Ok(response) => {
                println!("File found, serving 200 OK");
                response
            }
            Err(_) => {
                println!("File not found: {}, serving 404 page", file_path);
                Self::serve_error_page(error_page_path, 404, "Not Found", cookie)
            }
        }
    }
}

// Helper function to detect content type from file extension
pub fn detect_content_type(path: &str) -> &'static str {
    if let Some(ext) = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
    {
        match ext {
            "html" => "text/html",
            "css" => "text/css",
            "js" => "application/javascript",
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            _ => "application/octet-stream",
        }
    } else {
        "application/octet-stream"
    }
}

// === Handler functions for different HTTP methods ===

pub fn handle_method_not_allowed(
    allowed_methods: &[String],
    server: &ServerConfig,
    cookie: &Cookie,
) -> Vec<u8> {
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
            .cookie(cookie)
            .build(),
        Err(_) => HttpResponseBuilder::method_not_allowed()
            .header("Allow", &allow_header)
            .header("Content-Type", "text/plain")
            .body(b"Method Not Allowed".to_vec())
            .cookie(cookie)
            .build(),
    }
}

pub(crate) fn extract_multipart_files<'a>(
    body: &'a [u8],
    boundary: &str,
) -> Vec<(String, &'a [u8])> {
    let mut files = Vec::new();

    let boundary = format!("--{}", boundary);
    let boundary = boundary.as_bytes();

    let mut pos = 0;

    while let Some(b_start) = find_bytes(&body[pos..], boundary) {
        let part_start = pos + b_start + boundary.len();

        if part_start >= body.len() {
            break;
        }

        if let Some(b_end) = find_bytes(&body[part_start..], boundary) {
            let part = &body[part_start..part_start + b_end];
            pos = part_start + b_end;

            let header_end = find_bytes(part, b"\r\n\r\n");
            let Some(header_end) = header_end else {
                continue;
            };

            let headers = &part[..header_end];
            let data = &part[header_end + 4..];

            let headers_str = match std::str::from_utf8(headers) {
                Ok(s) => s,
                Err(_) => continue,
            };

            if !headers_str.contains("Content-Disposition") {
                continue;
            }

            let filename = extract_filename_from_disposition(headers_str);
            let Some(filename) = filename else { continue };

            let data = strip_trailing_crlf(data);

            files.push((filename, data));
        } else {
            break;
        }
    }

    files
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn strip_trailing_crlf(data: &[u8]) -> &[u8] {
    let mut end = data.len();
    if end >= 2 && &data[end - 2..end] == b"\r\n" {
        end -= 2;
    }
    data[..end].as_ref()
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

pub(crate) fn extract_boundary(content_type: &str) -> Option<String> {
    content_type
        .split(';')
        .find(|s| s.trim().starts_with("boundary="))
        .map(|s| s.trim().trim_start_matches("boundary=").to_string())
}

pub(crate) fn write_file(path: &str, data: &[u8], cookie: &Cookie        ) -> Vec<u8> {
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
            .cookie(cookie)
            .build(),
        Err(e) => HttpResponseBuilder::internal_error()
            .body(e.to_string().into_bytes())
            .cookie(cookie)
            .build(),
    }
}
