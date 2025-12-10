use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Debug)]
pub struct HttpRequestBuilder {
    buffer: Vec<u8>,
    request: HttpRequest,
    finished: bool,
}

impl HttpRequestBuilder {
    pub fn new() -> Self {
        Self { buffer: Vec::new(), request: HttpRequest { method: "GET".to_string(), path: "/".to_string(), headers: HashMap::new(), body: Vec::new() }, finished: false }
    }

    pub fn append(&mut self, chunk: Vec<u8>) -> Result<(), &'static str> {
        self.buffer.extend(chunk);
        if !self.buffer.is_empty() {
            self.request.body = self.buffer.clone();
            self.finished = true;
        }
        Ok(())
    }

    pub fn done(&self) -> bool { self.finished }

    pub fn get(&self) -> Option<HttpRequest> {
        if self.finished { Some(self.request.clone()) } else { None }
    }
}
