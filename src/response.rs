use std::fs;

use crate::{config::{Route, ServerConfig}, server, utils::HttpHeaders};

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

    // === File serving methods ===
    /// Serve a directory listing as HTML
    pub fn serve_directory_listing(dir_path: &str) -> Vec<u8> {
        let mut listing = String::from("<html><body><h1>Directory Listing</h1><ul>");

        if let Ok(entries) = fs::read_dir(dir_path) {
            for entry in entries {
                if let Ok(entry) = entry {
                    let file_name = entry.file_name();
                    let file_name_str = file_name.to_string_lossy();
                    listing.push_str(&format!(
                        "<li><a href=\"{}\">{}</a></li>",
                        file_name_str, file_name_str
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

pub fn handle_get(request_path: &str, server: &ServerConfig, route: &Route) -> Vec<u8> {
    if let Some(route) = server.routes.iter().find(|r| r.path == route.path) {
        // Directory listing allowed?
        if route.list_directory == Some(true) {

            println!("Serving directory  00000000000000000000000000000listing for: {}", route.root);
            return HttpResponseBuilder::serve_directory_listing(&route.root);
        }

        // Default file exists? Serve it
        if let Some(default_file) = &route.default_file {
            let root = &route.root;
            let full_path = format!("{}/{}", root, default_file);
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

pub fn handle_post(file_path: &str, body: &[u8]) -> Vec<u8> {
    // Example: Write/append to file
    match fs::write(file_path, body) {
        Ok(_) => {
            println!("POST: Successfully wrote to {}", file_path);
            HttpResponseBuilder::ok()
                .header("Content-Type", "text/plain")
                .body(b"File uploaded successfully".to_vec())
                .build()
        }
        Err(e) => {
            eprintln!("POST: Error writing to {}: {:?}", file_path, e);
            HttpResponseBuilder::internal_error()
                .header("Content-Type", "text/plain")
                .body(format!("Error: {}", e).into_bytes())
                .build()
        }
    }
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
