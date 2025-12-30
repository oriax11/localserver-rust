use crate::config::{Config, ServerConfig};
use crate::request::HttpRequestBuilder;
use crate::router::Router;
use mio::net::{TcpListener, TcpStream};
use mio::{Events, Interest, Poll, Token};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::time::Instant;
use std::path::Path;

const LISTENER_TOKEN_START: usize = 0;
const CONNECTION_TOKEN_START: usize = 10000;

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

// NEW: Track which listener accepted this connection
struct SocketData {
    stream: TcpStream,
    status: SocketStatus,
    listener_token: Token, // NEW: Remember which listener this came from
}

// NEW: Information about a listener and its associated servers
struct ListenerInfo {
    listener: TcpListener,
    host: String,
    port: u16,
    servers: Vec<ServerConfig>, // All servers that share this (host, port)
    default_server_index: usize, // Index into servers vec for default
}

pub struct Server {
    poll: Poll,
    events: Events,
    listeners: HashMap<Token, ListenerInfo>, // CHANGED: Store ListenerInfo instead of TcpListener
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
            next_token: CONNECTION_TOKEN_START,
        })
    }

    pub fn run(&mut self, config: Config) -> io::Result<()> {
        // Step 1: Group servers by (host, port)
        let mut listener_map: HashMap<(String, u16), Vec<(usize, ServerConfig)>> = HashMap::new();

        for (idx, server) in config.servers.iter().enumerate() {
            for &port in &server.ports {
                let key = (server.host.clone(), port);
                listener_map
                    .entry(key)
                    .or_insert_with(Vec::new)
                    .push((idx, server.clone()));
            }
        }

        // Step 2: Create one listener per unique (host, port)
        let mut token_counter = LISTENER_TOKEN_START;

        for ((host, port), server_list) in listener_map {
            println!("Setting up listener on {}:{}... ", host, port);
            let addr = format!("{}:{}", host, port).parse().unwrap();
            let mut listener = TcpListener::bind(addr)?;
            let token = Token(token_counter);
            token_counter += 1;

            self.poll
                .registry()
                .register(&mut listener, token, Interest::READABLE)?;

            // Determine default server: first one marked as default, or first in list
            let default_idx = server_list
                .iter()
                .position(|(_, srv)| srv.default_server)
                .unwrap_or(0);

            let servers: Vec<ServerConfig> = server_list.into_iter().map(|(_, srv)| srv).collect();

            println!(
                "Listening on {}:{} with {} server(s)",
                host,
                port,
                servers.len()
            );
            for (i, srv) in servers.iter().enumerate() {
                println!(
                    "  - {} {}",
                    srv.server_name,
                    if i == default_idx { "(default)" } else { "" }
                );
            }

            self.listeners.insert(
                token,
                ListenerInfo {
                    listener,
                    host,
                    port,
                    servers,
                    default_server_index: default_idx,
                },
            );
        }

        loop {
            self.poll.poll(&mut self.events, None)?;

            for event in self.events.iter() {
                let token = event.token();

                // Check if this is a listener token
                if token.0 < CONNECTION_TOKEN_START {
                    // Accept all incoming connections
                    if let Some(listener_info) = self.listeners.get_mut(&token) {
                        loop {
                            match listener_info.listener.accept() {
                                Ok((mut stream, _)) => {
                                    let conn_token = Token(self.next_token);
                                    self.next_token += 1;

                                    self.poll
                                        .registry()
                                        .register(
                                            &mut stream,
                                            conn_token,
                                            Interest::READABLE.add(Interest::WRITABLE),
                                        )
                                        .unwrap();

                                    self.connections.insert(
                                        conn_token,
                                        SocketData {
                                            stream,
                                            status: SocketStatus {
                                                ttl: Instant::now(),
                                                status: Status::Read,
                                                request: HttpRequestBuilder::new(),
                                                response: None,
                                            },
                                            listener_token: token, // NEW: Track which listener
                                        },
                                    );

                                    println!(
                                        "Accepted connection {:?} from listener {:?}",
                                        conn_token, token
                                    );
                                }
                                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                                Err(e) => {
                                    eprintln!("Accept error: {:?}", e);
                                    break;
                                }
                            }
                        }
                    }
                } else {
                    // Handle existing connection
                    if let Some(socket_data) = self.connections.get_mut(&token) {
                        loop {
                            // NEW: Pass listener_info for server selection
                            let listener_info = self.listeners.get(&socket_data.listener_token);
                            match Server::handle(socket_data, listener_info) {
                                Some(true) => continue, // State changed, keep going
                                Some(false) => {
                                    println!("Would block, waiting for next event.");
                                    break;
                                } // Would block, need event
                                None => {
                                    // Done/error
                                    let _ = socket_data.stream.shutdown(Shutdown::Both);
                                    self.connections.remove(&token);
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // CHANGED: Add listener_info parameter for server selection
    pub fn handle(
        socket_data: &mut SocketData,
        listener_info: Option<&ListenerInfo>,
    ) -> Option<bool> {
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
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                            return Some(false);
                        }
                        Err(_) => return None,
                    }
                }

                // Parse Host header and select the correct server
                let request = status_ref.request.get()?;

                // Extract hostname from Host header (strip port if present)
                let hostname = request
                    .headers
                    .get("host")
                    .map(|h| h.split(':').next().unwrap_or("").to_lowercase())
                    .unwrap_or_default();

                let selected_server = if let Some(info) = listener_info {
                    info.servers
                        .iter()
                        .find(|s| s.server_name.to_lowercase() == hostname)
                        .or_else(|| info.servers.get(info.default_server_index))
                } else {
                    None
                };

                let route = selected_server.and_then(|s| {
                    s.routes.iter().find(|r| {
                        request.path == r.path || request.path.starts_with(&(r.path.clone() + "/"))
                    })
                });

                let default_file = route
                    .and_then(|r| r.default_file.as_deref())
                    .unwrap_or("index.html");

                let doc_root = route
                    .and_then(|r| r.root.as_deref())
                    .or_else(|| selected_server.and_then(|s| s.root.as_deref()))
                    .unwrap_or("public");

                let path = if request.path == "/" {
                    format!("{}/{}", doc_root, default_file)
                } else {
                    format!("{}{}", doc_root, request.path)
                };

                println!("HOST      = {}", hostname);
                println!("DOC ROOT  = {}", doc_root);
                println!("FILE PATH= {}", path);

                let response_bytes = if path.ends_with(".py")
                    && route.as_ref().and_then(|r| r.cgi.as_ref()) == Some(&".py".to_string())
                {
                    crate::cgi::execute_cgi(request, &doc_root)
                } else if Path::new(&path).exists() {
                    let content = fs::read(&path).unwrap_or_default();
                    let mime = match path.rsplit('.').next() {
                        Some("html") => "text/html",
                        Some("css") => "text/css",
                        Some("js") => "application/javascript",
                        Some("png") => "image/png",
                        Some("jpg") | Some("jpeg") => "image/jpeg",
                        Some("gif") => "image/gif",
                        _ => "application/octet-stream",
                    };
                    let headers = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: {}\r\n\r\n",
                        content.len(),
                        mime
                    );
                    let mut bytes = headers.into_bytes();
                    bytes.extend_from_slice(&content);
                    bytes
                } else {
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec()
                };

                status_ref.response = Some(Box::new(SimpleResponse::new(response_bytes)));
                status_ref.status = Status::Write;
                println!("Serving path: {} for Host : {}", path, hostname);
                Some(true)
            }

            Status::Write => {
                if let Some(response) = &mut status_ref.response {
                    loop {
                        let data = response.peek();
                        if data.is_empty() {
                            break; // tout envoyé
                        }
                        match socket_data.stream.write(data) {
                            Ok(n) => response.next(n),
                            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                                return Some(false); // attendre prochain événement
                            }
                            Err(_) => return None, // erreur -> fermer connexion
                        }
                    }

                    // Ici, tout est écrit
                    let keep_alive = status_ref
                        .request
                        .get()
                        .and_then(|req| req.headers.get("connection"))
                        .map(|v| v.to_lowercase() == "keep-alive")
                        .unwrap_or(false);

                    if keep_alive {
                        // Réinitialiser pour lire une nouvelle requête sur la même connexion
                        status_ref.status = Status::Read;
                        status_ref.request = HttpRequestBuilder::new();
                        status_ref.response = None;
                    } else {
                        // Fermer la connexion
                        let _ = socket_data.stream.shutdown(Shutdown::Both);
                        return None;
                    }
                }
                Some(true)
            }

            Status::Finish => {
                return None;
            }
        }
    }
}
