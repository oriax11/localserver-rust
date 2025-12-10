use crate::config::Config;
use crate::request::HttpRequestBuilder;
use crate::router::Router;
use mio::net::{TcpListener, TcpStream};
use mio::{Events, Interest, Poll, Token};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Write};
use std::time::Instant;
use std::net::Shutdown;

const SERVER_TOKEN: Token = Token(0);

pub trait HttpResponseCommon {
    fn peek(&self) -> &[u8];
    fn next(&mut self, n: usize);
    fn is_finished(&self) -> bool;
}

pub struct SimpleResponse {
    data: Vec<u8>,
    index: usize,
}

impl SimpleResponse {
    pub fn new(data: Vec<u8>) -> Self {
        Self { data, index: 0 }
    }
}

impl HttpResponseCommon for SimpleResponse {
    fn peek(&self) -> &[u8] {
        &self.data[self.index..]
    }

    fn next(&mut self, n: usize) {
        self.index += n;
    }

    fn is_finished(&self) -> bool {
        self.index >= self.data.len()
    }
}

#[derive(PartialEq, Debug)]
enum Status {
    Read,
    Write,
    Finish,
}

struct SocketStatus {
    ttl: Instant,
    status: Status,
    request: HttpRequestBuilder,
    response: Option<Box<dyn HttpResponseCommon>>,
}

struct SocketData {
    stream: TcpStream,
    status: SocketStatus,
}

pub struct Server {
    poll: Poll,
    events: Events,
    listeners: HashMap<Token, TcpListener>,
    connections: HashMap<Token, SocketData>,
    router: Router,
    next_token: usize,
}

impl Server {
    pub fn new() -> io::Result<Self> {
        Ok(Server {
            poll: Poll::new()?,
            events: Events::with_capacity(1024),
            listeners: HashMap::new(),
            connections: HashMap::new(),
            router: Router::new(),
            next_token: 1,
        })
    }

    pub fn run(&mut self, config: Config) -> io::Result<()> {
        let server_config = &config.servers[0];
        let addr = format!("{}:{}", server_config.host, server_config.ports[0])
            .parse()
            .unwrap();

        let mut listener = TcpListener::bind(addr)?;
        self.poll
            .registry()
            .register(&mut listener, SERVER_TOKEN, Interest::READABLE)?;
        self.listeners.insert(SERVER_TOKEN, listener);

        println!("Server listening on {}", addr);

        loop {
            self.poll.poll(&mut self.events, None)?;

            for event in self.events.iter() {
                match event.token() {
                    SERVER_TOKEN => {
                        // Accept all incoming connections
                        let listener = self.listeners.get_mut(&SERVER_TOKEN).unwrap();
                        loop {
                            match listener.accept() {
                                Ok((mut stream, _)) => {
                                    let token = Token(self.next_token);
                                    self.next_token += 1;

                                    self.poll
                                        .registry()
                                        .register(
                                            &mut stream,
                                            token,
                                            Interest::READABLE.add(Interest::WRITABLE),
                                        )
                                        .unwrap();

                                    self.connections.insert(
                                        token,
                                        SocketData {
                                            stream,
                                            status: SocketStatus {
                                                ttl: Instant::now(),
                                                status: Status::Read,
                                                request: HttpRequestBuilder::new(),
                                                response: None,
                                            },
                                        },
                                    );

                                    println!("Accepted connection {:?}", token);
                                }
                                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                                Err(e) => {
                                    eprintln!("Accept error: {:?}", e);
                                    break;
                                }
                            }
                        }
                    }
                    token => {
                        if let Some(socket_data) = self.connections.get_mut(&token) {
                            if Server::handle(socket_data).is_none() {
                                let _ = socket_data.stream.shutdown(Shutdown::Both);
                                self.connections.remove(&token);
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn handle(socket_data: &mut SocketData) -> Option<()> {
        let status_ref = &mut socket_data.status;

        match status_ref.status {
            Status::Read => {
                let mut buf = [0u8; 2048];
                loop {
                    match socket_data.stream.read(&mut buf) {
                        Ok(0) => return None,
                        Ok(n) => {
                            let _ = status_ref.request.append(buf[..n].to_vec());
                            if status_ref.request.done() {
                                break;
                            }
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => return Some(()),
                        Err(_) => return None,
                    }
                }

                // Préparer la réponse HTTP
                let request = status_ref.request.get()?;
                let path = if request.path == "/" {
                    "public/index.html".to_string()
                } else {
                    format!("public{}", request.path)
                };

                let response_bytes = match fs::read(&path) {
                    Ok(content) => {
                        let mut headers = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\n\r\n",
                            content.len()
                        )
                        .into_bytes();
                        headers.extend_from_slice(&content);
                        headers
                    }
                    Err(_) => b"HTTP/1.1 404 Not Found\r\n\r\n".to_vec(),
                };

                status_ref.response = Some(Box::new(SimpleResponse::new(response_bytes)));
                status_ref.status = Status::Write;
                Some(())
            }

            Status::Write => {
                if let Some(response) = &mut status_ref.response {
                    loop {
                        let data = response.peek();
                        if data.is_empty() {
                            break;
                        }
                        match socket_data.stream.write(data) {
                            Ok(n) => response.next(n),
                            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => return Some(()),
                            Err(_) => return None,
                        }
                    }

                    if response.is_finished() {
                        let request = status_ref.request.get()?;
                        let keep_alive = request
                            .headers
                            .get("connection")
                            .map(|v| v.to_lowercase() == "keep-alive")
                            .unwrap_or(false);

                        if keep_alive {
                            status_ref.status = Status::Read;
                            status_ref.request = HttpRequestBuilder::new();
                            status_ref.response = None;
                        } else {
                            let _ = socket_data.stream.shutdown(Shutdown::Both);
                            return None;
                        }
                    }
                }
                Some(())
            }

            Status::Finish => None,
        }
    }
}
