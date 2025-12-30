use std::collections::HashMap;

#[derive(Debug)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub version: String,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
}

#[derive(Debug)]
pub struct HttpRequestBuilder {
    buffer: Vec<u8>,
    request: Option<HttpRequest>,
}

impl HttpRequestBuilder {
    pub fn new() -> Self {
        Self { buffer: Vec::new(), request: None }
    }

    pub fn append(&mut self, data: Vec<u8>) -> Result<(), &'static str> {
        self.buffer.extend(data);

        if self.done() {
            let s = String::from_utf8_lossy(&self.buffer);
            let mut lines = s.lines();
            if let Some(request_line) = lines.next() {
                let parts: Vec<&str> = request_line.split_whitespace().collect();
                if parts.len() != 3 {
                    return Err("Invalid request line");
                }

                let mut headers = HashMap::new();
                for line in lines.clone() {
                    let line = line.trim();
                    if line.is_empty() { break; }
                    if let Some((key, val)) = line.split_once(":") {
                        headers.insert(key.to_lowercase(), val.trim().to_string());
                    }
                }

                // Calculer body si Content-Length présent
                let content_length = headers
                    .get("content-length")
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(0);

                let header_end = self.buffer.windows(4)
                    .position(|w| w == b"\r\n\r\n")
                    .unwrap() + 4;

                let body_bytes = if self.buffer.len() >= header_end + content_length {
                    Some(self.buffer[header_end..header_end + content_length].to_vec())
                } else if content_length > 0 {
                    // Body pas encore complet
                    return Ok(());
                } else {
                    None
                };

                self.request = Some(HttpRequest {
                    method: parts[0].to_string(),
                    path: parts[1].to_string(),
                    version: parts[2].to_string(),
                    headers,
                    body: body_bytes,
                });
            }
        }

        Ok(())
    }

    pub fn done(&self) -> bool {
        // Vérifie si headers complets
        if self.buffer.windows(4).any(|w| w == b"\r\n\r\n") {
            if let Some(request) = &self.request {
                if let Some(cl) = request.headers.get("content-length") {
                    if let Ok(len) = cl.parse::<usize>() {
                        let header_end = self.buffer.windows(4)
                            .position(|w| w == b"\r\n\r\n")
                            .unwrap() + 4;
                        return self.buffer.len() >= header_end + len;
                    }
                }
            }
            return true;
        }
        false
    }

    pub fn get(&self) -> Option<&HttpRequest> {
        self.request.as_ref()
    }
}
