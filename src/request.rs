use crate::utils::{HttpHeaders, HttpMethod};

#[derive(Debug)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub path: String,
    pub version: String,
    pub headers: HttpHeaders,
    pub body: Option<Vec<u8>>,
}

#[derive(Debug)]
enum ParserState {
    ParsingHeaders,
    ParsingBody {
        headers_end: usize,
        body_type: BodyType,
    },
    Complete,
}

#[derive(Debug)]
enum BodyType {
    ContentLength(usize),
    Chunked {
        bytes_read: usize,
        current_chunk_size: Option<usize>,
        current_chunk_read: usize,
    },
    None,
}

#[derive(Debug)]
pub struct HttpRequestBuilder {
    buffer: Vec<u8>,
    state: ParserState,
    request: Option<HttpRequest>,
}

impl HttpRequestBuilder {
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            state: ParserState::ParsingHeaders,
            request: None,
        }
    }

    pub fn append(&mut self, data: Vec<u8>) -> Result<(), &'static str> {
        self.buffer.extend(data);

        match &self.state {
            ParserState::ParsingHeaders => {
                if let Some(headers_end) = self.find_headers_end() {
                    self.parse_headers(headers_end)?;
                }
            }
            ParserState::ParsingBody { .. } => {
                self.parse_body()?;
            }
            ParserState::Complete => {}
        }

        Ok(())
    }

    fn find_headers_end(&self) -> Option<usize> {
        if let Some(pos) = self.buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            return Some(pos);
        }

        if let Some(pos) = self.buffer.windows(2).position(|w| w == b"\n\n") {
            return Some(pos);
        }

        None
    }

    fn parse_headers(&mut self, headers_end: usize) -> Result<(), &'static str> {
        let headers_section = &self.buffer[..headers_end];
        let s = String::from_utf8_lossy(headers_section);
        let mut lines = s.lines();

        let request_line = lines.next().ok_or("Missing request line")?;
        let parts: Vec<&str> = request_line.split_whitespace().collect();
        if parts.len() != 3 {
            return Err("Invalid request line");
        }

        let mut headers = HttpHeaders::new();
        for line in lines {
            let line = line.trim();
            if line.is_empty() {
                break;
            }
            if let Some((key, val)) = line.split_once(":") {
                headers.insert(key, val);
            }
        }
        // CHECK FOR CONNECTION  IF NULL ADD KEEP ALIVE BY DEFAULT
        if !headers.get("connection").is_some() {
            headers.insert("connection", "keep-alive");
        }

        let body_type = self.determine_body_type(&headers);

        self.request = Some(HttpRequest {
            method: HttpMethod::from_str(parts[0]),
            path: parts[1].to_string(),
            version: parts[2].to_string(),
            headers,
            body: None,
        });

        self.state = ParserState::ParsingBody {
            headers_end,
            body_type,
        };

        self.parse_body()?;

        Ok(())
    }

    fn determine_body_type(&self, headers: &HttpHeaders) -> BodyType {
        if let Some(transfer_encoding) = headers.get("transfer-encoding") {
            if transfer_encoding.to_lowercase().contains("chunked") {
                return BodyType::Chunked {
                    bytes_read: 0,
                    current_chunk_size: None,
                    current_chunk_read: 0,
                };
            }
        }

        if let Some(content_length) = headers.get("content-length") {
            if let Ok(length) = content_length.trim().parse::<usize>() {
                return BodyType::ContentLength(length);
            }
        }

        BodyType::None
    }

    fn parse_body(&mut self) -> Result<(), &'static str> {
        let (headers_end, body_type) = match &self.state {
            ParserState::ParsingBody {
                headers_end,
                body_type,
            } => (*headers_end, body_type),
            _ => return Ok(()),
        };

        match body_type {
            BodyType::None => {
                self.state = ParserState::Complete;
                Ok(())
            }
            BodyType::ContentLength(expected_length) => {
                let body_start = headers_end;
                let available = self.buffer.len().saturating_sub(body_start);

                if available >= *expected_length {
                    let body = self.buffer[body_start..body_start + expected_length].to_vec();
                    if let Some(ref mut req) = self.request {
                        req.body = Some(body);
                    }
                    self.state = ParserState::Complete;
                }
                Ok(())
            }
            BodyType::Chunked { .. } => self.parse_chunked_body(headers_end),
        }
    }

    fn parse_chunked_body(&mut self, headers_end: usize) -> Result<(), &'static str> {
        let mut body_data = Vec::new();
        let mut pos = headers_end;

        loop {
            // Find chunk size line ending
            let chunk_header_end = self.buffer[pos..]
                .windows(2)
                .position(|w| w == b"\r\n")
                .map(|p| pos + p);

            let chunk_header_end = match chunk_header_end {
                Some(end) => end,
                None => return Ok(()), // Need more data for chunk size
            };

            // Parse chunk size
            let chunk_size_str = String::from_utf8_lossy(&self.buffer[pos..chunk_header_end]);
            let chunk_size_str = chunk_size_str.split(';').next().unwrap_or("").trim();
            let chunk_size =
                usize::from_str_radix(chunk_size_str, 16).map_err(|_| "Invalid chunk size")?;

            // Move past chunk size line
            pos = chunk_header_end + 2;

            if chunk_size == 0 {
                // Last chunk - check for trailing \r\n
                if self.buffer.len() >= pos + 2 {
                    if let Some(ref mut req) = self.request {
                        req.body = Some(body_data);
                    }
                    self.state = ParserState::Complete;
                    return Ok(());
                } else {
                    return Ok(()); // Need more data for final \r\n
                }
            }

            // Check if we have the full chunk + trailing \r\n
            if self.buffer.len() < pos + chunk_size + 2 {
                return Ok(()); // Need more data
            }

            // Read chunk data
            body_data.extend_from_slice(&self.buffer[pos..pos + chunk_size]);
            pos += chunk_size + 2; // Skip chunk data and trailing \r\n
        }
    }

    pub fn done(&self) -> bool {
        matches!(self.state, ParserState::Complete)
    }

    pub fn get(&self) -> Option<&HttpRequest> {
        if self.done() {
            self.request.as_ref()
        } else {
            None
        }
    }
}
