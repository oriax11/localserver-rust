use crate::utils::HttpHeaders;
use crate::utils::HttpMethod;
use std::{cmp::min, collections::HashMap, net::TcpStream, str};

const MAX_HEADER_SIZE: usize = 1024 * 16;
const MAX_HEADER_COUNT: usize = 100;

#[derive(Debug)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub path: String,
    pub args: HashMap<String, String>,
    pub headers: HttpHeaders,
    pub body: Vec<u8>,
}

#[derive(Debug)]

pub struct HttpRequestBuilder {
    request: HttpRequest,
    buffer: Vec<u8>,
    state: State,
    body_size: usize,
    chunked: bool,
}

#[derive(Debug, PartialEq)]
enum State {
    Init,
    Headers,
    Body,
    Finish,
}

impl HttpRequestBuilder {
    /// Creates a new builder in the initial state.
    pub fn new() -> Self {
        return HttpRequestBuilder {
            request: HttpRequest {
                method: HttpMethod::GET,
                path: String::new(),
                args: HashMap::new(),
                headers: HttpHeaders::new(),
                body: Vec::new(),
            },
            chunked: false,
            state: State::Init,
            body_size: 0,
            buffer: Vec::new(),
        };
    }

    // /// Returns the built request if parsing is complete.
    // pub fn get(&self) -> Option<HttpRequest> {
    //     match self.state {
    //         State::Finish => Some(self.request),
    //         _ => None,
    //     }
    // }

    pub fn done(&self) -> bool {
        self.state == State::Finish
    }

    pub fn append(&mut self, chunk: Vec<u8>) -> Result<(), &'static str> {
        self.buffer.extend(chunk);

        while !self.buffer.is_empty() {
            match self.state {
                State::Init => {
                    println!(" INIT");
                    let line = get_line(&mut self.buffer);
                    if line.is_none() {
                        if self.buffer.len() >= MAX_HEADER_SIZE {
                            return Err("Entity Too Large");
                        }
                        return Ok(());
                    }
                    let line = line.unwrap();
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() != 3 {
                        return Err("Invalid method + path + version request");
                    }
                    self.request.method = HttpMethod::from_str(parts[0]);
                    let path_parts: Vec<&str> = parts[1].split('?').collect();
                    self.request.path = path_parts[0].to_string();
                    if path_parts.len() > 1 {
                        self.request.args = path_parts[1]
                            .split('&')
                            .filter_map(|pair| {
                                let kv: Vec<&str> = pair.split('=').collect();
                                if kv.len() == 2 {
                                    Some((kv[0].to_string(), kv[1].to_string()))
                                } else {
                                    None
                                }
                            })
                            .collect();
                    }
                    self.state = State::Headers;
                }
                State::Headers => {
                    println!(" HEADERS");

                    let line = get_line(&mut self.buffer);
                    if line.is_none() {
                        return Ok(());
                    }
                    let line = line.unwrap();
                    if line.is_empty() {
                        
                        self.state = if self.body_size == 0 && !self.chunked {
                            State::Finish
                        } else {
                            State::Body
                        };
                        println!("{:#?}", self.state);
                        continue;
                    }
                    if self.request.headers.len() > MAX_HEADER_COUNT || line.len() > MAX_HEADER_SIZE
                    {
                        return Err("Header number exceed allowed");
                    }

                    let (key, value) = line.split_once(':').ok_or("Invalid Header")?;

                    let key = key.trim();
                    let value = value.trim();
                    if key.to_lowercase() == "content-length" {
                        if self.request.headers.get("content-length").is_some()
                            || self.request.headers.get("Transfer-Encoding")
                                == Some(&"chunked".to_string())
                        {
                            continue;
                        }
                        self.body_size = value
                            .parse::<usize>()
                            .map_err(|_| "invalid content-length")?;
                    }
                    if key.to_lowercase() == "transfer-encoding"
                        && value.to_lowercase() == "chunked"
                    {
                        if self.request.headers.get("content-length").is_some() {
                            continue;
                        }
                        self.chunked = true;
                    }
                    self.request.headers.insert(key, value);
                }
                State::Body => {
                    print!("boDYa");
                    let body_left = self.body_size - self.request.body.len();
                    if body_left > 0 {
                        let to_take = min(body_left, self.buffer.len());
                        let to_append = self.buffer.drain(..to_take);
                        let to_append = to_append.as_slice();
                        self.request.body.extend_from_slice(to_append);
                    }
                    if self.chunked {
                        if self.body_size != 0 {
                            let empty = get_line(&mut self.buffer);
                            if empty.is_none() {
                                return Ok(());
                            }
                        }
                        let size = get_line(&mut self.buffer);
                        if size.is_none() {
                            return Ok(());
                        }
                        let size = size.unwrap();
                        let size = size.strip_prefix("0x").unwrap_or(&size);
                        let size =
                            i64::from_str_radix(size, 16).map_err(|_| "Invalud chunk size")?;
                        if size == 0 {
                            self.state = State::Finish;
                            return Ok(());
                        }
                        self.body_size += size as usize;
                    } else {
                        self.state = State::Finish;
                        return Ok(());
                    }
                }
                State::Finish => return Ok(()),
            }
        }

        Ok(())
    }
}

fn get_line(buffer: &mut Vec<u8>) -> Option<String> {
    if let Some(pos) = buffer.windows(2).position(|w| w == b"\r\n") {
        let line = buffer.drain(..pos).collect::<Vec<u8>>();
        buffer.drain(..2); // remove CRLF
        return match str::from_utf8(line.as_slice()) {
            Ok(v) => Some(v.to_string()),
            Err(_e) => None,
        };
    }
    None
}
