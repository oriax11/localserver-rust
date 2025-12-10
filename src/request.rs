use std::collections::HashMap;

#[derive(Debug)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub version: String,
    pub headers: HashMap<String, String>,
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
                for line in lines {
                    let line = line.trim();
                    if line.is_empty() { break; }
                    if let Some((key, val)) = line.split_once(":") {
                        headers.insert(key.to_lowercase(), val.trim().to_string());
                    }
                }
                self.request = Some(HttpRequest {
                    method: parts[0].to_string(),
                    path: parts[1].to_string(),
                    version: parts[2].to_string(),
                    headers,
                });
            }
        }
        Ok(())
    }

    pub fn done(&self) -> bool {
        self.buffer.windows(4).any(|w| w == b"\r\n\r\n")
    }

    pub fn get(&self) -> Option<&HttpRequest> {
        self.request.as_ref()
    }
}
