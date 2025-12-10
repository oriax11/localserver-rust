use crate::config::Config;
use crate::request::HttpRequestBuilder;
use crate::router::Router;
use crate::response::{HttpResponseCommon, SimpleResponse};
use mio::net::{TcpListener, TcpStream};
use mio::{Events, Interest, Poll, Token};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::time::Instant;

const SERVER_TOKEN: Token = Token(0);

#[derive(PartialEq, Debug)]
enum Status { Read, Write, Finish }

struct SocketStatus {
    ttl: Instant,
    status: Status,
    request: HttpRequestBuilder,
    index_writed: usize,
    response: Option<Box<dyn HttpResponseCommon>>,
}
struct SocketData {
    stream: TcpStream,
    status: Option<SocketStatus>,
}

pub struct Server {
    pub router: Router,
    poll: Poll,
    events: Events,
    listeners: HashMap<Token, TcpListener>,
    connections: HashMap<Token, SocketData>,
    next_token: usize,
}

impl Server {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            router: Router::new(),
            poll: Poll::new()?,
            events: Events::with_capacity(1024),
            listeners: HashMap::new(),
            connections: HashMap::new(),
            next_token: 1,
        })
    }

    pub fn run(&mut self, config: Config) -> io::Result<()> {
        let server_config = &config.servers[0];
        let addr = format!("{}:{}", server_config.host, server_config.ports[0])
            .parse()
            .unwrap();

        let mut main_listener = TcpListener::bind(addr)?;
        self.poll.registry().register(&mut main_listener, SERVER_TOKEN, Interest::READABLE)?;
        self.listeners.insert(SERVER_TOKEN, main_listener);

        println!("Server listening on {}", addr);

        loop {
            self.poll.poll(&mut self.events, None)?;
            for event in self.events.iter() {
                match event.token() {
                    SERVER_TOKEN => {
                        loop {
                            match self.listeners.get_mut(&SERVER_TOKEN).unwrap().accept() {
                                Ok((mut stream, _)) => {
                                    let token = Token(self.next_token);
                                    self.next_token += 1;
                                    self.poll.registry().register(
                                        &mut stream, token, Interest::READABLE.add(Interest::WRITABLE)
                                    )?;
                                    let socket_status = SocketStatus {
                                        ttl: Instant::now(),
                                        status: Status::Read,
                                        request: HttpRequestBuilder::new(),
                                        index_writed: 0,
                                        response: None,
                                    };
                                    let socket_data = SocketData { stream, status: Some(socket_status) };
                                    self.connections.insert(token, socket_data);
                                    println!("Accepted new connection {:?}", token);
                                }
                                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                                Err(e) => {
                                    eprintln!("Accept error: {}", e);
                                    break;
                                }
                            }
                        }
                    }
                    token => {
                        if let Some(socket_data) = self.connections.get_mut(&token) {
                            let _ = Self::handle(socket_data, &self.router);
                        }
                    }
                }
            }
        }
    }

    pub fn handle(socket_data: &mut SocketData, router: &Router) -> Option<()> {
        let status_ref = socket_data.status.as_mut()?;
        match status_ref.status {
            Status::Read => {
                let mut buffer = [0; 2048];
                match socket_data.stream.read(&mut buffer) {
                    Ok(0) => return None,
                    Ok(n) => {
                        let _ = status_ref.request.append(buffer[..n].to_vec());
                        let request = status_ref.request.get()?;
                        if crate::cgi::is_cgi_request(&request.path) {
                            let cgi_bytes = crate::cgi::execute_cgi(&request);
                            status_ref.response = Some(Box::new(SimpleResponse::new(cgi_bytes)));
                        } else if let Some(handler) = router.route(&request.path) {
                            let response = handler(&request);
                            status_ref.response = Some(Box::new(SimpleResponse::new(response.to_bytes())));
                        } else {
                            status_ref.response = Some(Box::new(SimpleResponse::new(
                                b"HTTP/1.1 404 Not Found\r\n\r\n".to_vec()
                            )));
                        }
                        status_ref.status = Status::Write;
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => return Some(()),
                    Err(_) => return None,
                }
            }
            Status::Write => {
                if let Some(response) = &mut status_ref.response {
                    loop {
                        let data = response.peek();
                        if data.is_empty() { break; }
                        match socket_data.stream.write(data) {
                            Ok(_) => response.next(),
                            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => return Some(()),
                            Err(_) => return None,
                        }
                    }
                    if response.is_finished() {
                        let request = status_ref.request.get()?;
                        let keep_alive = request.headers.get("connection")
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
            }
            Status::Finish => return None,
        }
        Some(())
    }
}
