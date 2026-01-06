use crate::error::get_error_page_path;
use crate::utils::cookie::{self, Cookie};
use crate::{
    config::ServerConfig,
    request::HttpRequest,
    response::{HttpResponseBuilder, extract_boundary, extract_multipart_files, write_file},
    server::{FileResponse, HttpResponseCommon, SimpleResponse},
};
use std::fs;
use uuid::Uuid;

pub fn handle_get(
    request_path: &str,
    server: &ServerConfig,
    request: &HttpRequest,
    cookie: &Cookie,
) -> Box<dyn HttpResponseCommon> {
    // Trim request path from leading/trailing '/'
    let path = request.path.trim_matches('/');

    if let Some(route) = server
        .routes
        .iter()
        .find(|r| r.path.trim_matches('/') == path)
    {
        // Directory listing allowed?
        if route.list_directory == Some(true) {
            let content = HttpResponseBuilder::serve_directory_listing(
                &server.root,
                &route.root,
                &route.path,
                &cookie,
            );
            return Box::new(SimpleResponse::new(content));
        }

        // Default file exists? Serve it
        if let Some(default_file) = &route.default_file {
            let (key, value) = cookie.to_header_pair();

            let server_root = &server.root;
            let root = &route.root;
            let full_path = format!("{}/{}/{}", server_root, root, default_file);
            match FileResponse::new(&full_path) {
                Ok(file_response) => {
                    return Box::new(file_response);
                }
                Err(_) => {
                    return Box::new(SimpleResponse::new(
                        HttpResponseBuilder::not_found()
                            .header(&key, &value)
                            .build(),
                    ));
                }
            }
        }
    }

    // If no route/default file, serve requested file directly
    match FileResponse::new(&request_path) {
        Ok(file_response) => {
            return Box::new(file_response);
        }
        Err(_) => {
            return Box::new(SimpleResponse::new(
                HttpResponseBuilder::not_found().build(),
            ));
        }
    }
}

pub fn handle_delete(file_path: &str, error_page_path: &str , cookie :&Cookie) -> Vec<u8> {
    match fs::remove_file(file_path) {
        Ok(_) => {
            println!("DELETE: Successfully deleted {}", file_path);
            HttpResponseBuilder::no_content().build()
        }
        Err(_) => {
            println!("DELETE: File not found {}", file_path);
            HttpResponseBuilder::serve_error_page(error_page_path, 404, "Not Found", cookie)
        }
    }
}

pub fn handle_post(file_path: &str, request: &HttpRequest , cookie: &Cookie) -> Vec<u8> {
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
        || content_type.starts_with("font/")
        || content_type.starts_with("text/")
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

        println!(
            "Direct upload filename: {}  and  save path is  {}",
            filename, file_path
        );

        let save_path = if file_path.ends_with('/') {
            format!("{}{}", file_path, filename)
        } else {
            format!("{}", file_path)
        };

        return write_file(&save_path, body, cookie);
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
            println!("No files extracted from multipart body");
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

            let response = write_file(&save_path, file_bytes , cookie);
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
