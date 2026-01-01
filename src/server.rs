use crate::config::{Config, Route, ServerConfig};
use crate::request::HttpRequestBuilder;
use crate::response::{
    HttpResponseBuilder, handle_delete, handle_get, handle_method_not_allowed, handle_post,
};
use crate::router::Router;
use crate::utils::{HttpHeaders, HttpMethod};
use mio::net::{TcpListener, TcpStream};
use mio::{Events, Interest, Poll, Token};
use std::collections::HashMap;
use std::f32::consts::E;
use std::fs;
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::time::Instant;

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

//////////////////////////
fn build_http_response(
    status_code: u16,
    status_text: &str,
    content: Vec<u8>,
    content_type: &str,
) -> Vec<u8> {
    let mut headers = format!(
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nContent-Type: {}\r\n\r\n",
        status_code,
        status_text,
        content.len(),
        content_type
    )
    .into_bytes();
    headers.extend_from_slice(&content);
    headers
}

fn build_404_response(error_page_path: &str) -> Vec<u8> {
    match fs::read(error_page_path) {
        Ok(content) => {
            println!("Serving custom 404 error page from: {}", error_page_path);
            build_http_response(404, "Not Found", content, "text/html")
        }
        Err(e) => {
            println!(
                "Error page '{}' not found, sending minimal 404 response. [Error: {:?}]",
                error_page_path, e
            );
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec()
        }
    }
}

fn serve_file_or_404(file_path: &str, error_page_path: &str) -> Vec<u8> {
    println!("Attempting to serve file: {}", file_path);

    match fs::read(file_path) {
        Ok(content) => {
            println!("File found, serving 200 OK");
            build_http_response(200, "OK", content, "text/html")
        }
        Err(_) => {
            println!("File not found: {}, serving 404 page", file_path);
            build_404_response(error_page_path)
        }
    }
}

/// Extracts the hostname from the Host header (strips port if present)
fn extract_hostname(headers: &HttpHeaders) -> &str {
    headers
        .get("host")
        .and_then(|h| h.split(':').next())
        .unwrap_or("")
}

fn select_server<'a>(listener_info: &'a ListenerInfo, hostname: &str) -> Option<&'a ServerConfig> {
    // Try to find a server matching the hostname
    let matched = listener_info
        .servers
        .iter()
        .find(|s| s.server_name == hostname);

    if let Some(srv) = matched {
        println!(
            "Selected server '{}' for Host: {}",
            srv.server_name, hostname
        );
        Some(srv)
    } else {
        let default = listener_info
            .servers
            .get(listener_info.default_server_index);
        if let Some(srv) = default {
            println!(
                "No match for Host: '{}', using default server '{}'",
                hostname, srv.server_name
            );
        }
        default
    }
}

fn get_error_page_path(server: Option<&ServerConfig>, status_code: u16) -> String {
    server
        .and_then(|s| {
            s.error_pages
                .iter()
                .find(|ep| ep.code == status_code)
                .map(|ep| ep.path.clone())
        })
        .unwrap_or_else(|| format!("./error_pages/{}.html", status_code))
}

fn find_matching_route<'a>(server: &'a ServerConfig, request_path: &str) -> Option<&'a Route> {
    server
        .routes
        .iter()
        .filter(|route| {
            if route.path == "/" {
                true
            } else {
                request_path == route.path || request_path.starts_with(&(route.path.clone() + "/"))
            }
        })
        .max_by_key(|route| route.path.len())
}

fn resolve_file_path(
    server: Option<&ServerConfig>,
    route: &crate::config::Route,
    request_path: &str,
) -> String {
    let server_root = server.and_then(|s| s.root.as_deref()).unwrap_or(".");
    let route_root = route.root.as_deref().unwrap_or("");
    let base = format!("{}/{}", server_root, route_root);

    if request_path == route.path {
        if let Some(index) = &route.default_file {
            format!("{}/{}", base, index)
        } else {
            base
        }
    } else {
        let suffix = request_path.strip_prefix(&route.path).unwrap_or("");
        format!("{}/{}", base, suffix)
    }
}
//////

/// Returns Some(true) if request is complete, Some(false) if would block, None on error
fn read_request(stream: &mut TcpStream, request: &mut HttpRequestBuilder) -> Option<bool> {
    let mut buf = [0u8; 2048];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => return None, // Connection closed
            Ok(n) => {

                if let Err(e) = request.append(buf[..n].to_vec()) {
                    return None;
                }
                if request.done() {
                    return Some(true); // Request complete
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                return Some(false); // Would block, need more data
            }
            Err(_) => return None, // Error
        }
    }
}

/// Writes response data to the stream
/// Returns Some(true) if should continue, Some(false) if would block, None on error
fn write_response(
    stream: &mut TcpStream,
    response: &mut Box<dyn HttpResponseCommon>,
) -> Option<bool> {
    loop {
        let data = response.peek();
        if data.is_empty() {
            return Some(true); // Write complete
        }
        match stream.write(data) {
            Ok(n) => response.next(n),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                return Some(false); // Would block
            }
            Err(_) => return None, // Error
        }
    }
}

/// Checks if the connection should be kept alive based on headers
fn should_keep_alive(request: &crate::request::HttpRequest) -> bool {
    request
        .headers
        .get("connection")
        .map(|v| v.to_lowercase() == "keep-alive")
        .unwrap_or(false)
}

//////////////////////////
///
///
///
///
fn handle_read_state(
    socket_data: &mut SocketData,
    listener_info: Option<&ListenerInfo>,
) -> Option<bool> {
    // Read the request
    let read_result = read_request(&mut socket_data.stream, &mut socket_data.status.request);

    match read_result {
        Some(true) => {}       // Request complete, continue processing
        other => return other, // Would block or error
    }

    // Parse request
    let request = socket_data.status.request.get()?;

    // Select server based on Host header
    let hostname = extract_hostname(&request.headers);
    let selected_server = listener_info.and_then(|info| select_server(info, hostname));

    // Get error page paths
    let error_404_path = get_error_page_path(selected_server, 404);
    let error_405_path = get_error_page_path(selected_server, 405);

    // Find matching route
    let selected_route =
        selected_server.and_then(|server| find_matching_route(server, &request.path));

    let response_bytes = match selected_route {
        Some(route) => {
            // Check if method is allowed
            let request_method = &request.method;
            let method_allowed = route
                .methods
                .iter()
                .any(|m| HttpMethod::from_str(m) == *request_method);

            if !method_allowed {
                let allowed: Vec<String> = route.methods.clone();

                handle_method_not_allowed(&allowed, &error_405_path)
            } else {
                // Resolve file path
                let file_path = resolve_file_path(selected_server, route, &request.path);

                // Handle based on method
                match request_method {
                    HttpMethod::GET => handle_get(&file_path, &error_404_path),
                    HttpMethod::POST => {
                        let body = request.body.as_deref().unwrap_or(&[]);
                        handle_post(&file_path, body, &error_404_path)
                    }
                    HttpMethod::DELETE => handle_delete(&file_path, &error_404_path),
                    HttpMethod::Other(_) => handle_method_not_allowed(&[], &error_405_path),
                }
            }
        }
        None => {
            HttpResponseBuilder::serve_error_page(&error_404_path, 404, "Not Found")
        }
    };

    // Set response and transition to Write state
    socket_data.status.response = Some(Box::new(SimpleResponse::new(response_bytes)));
    socket_data.status.status = Status::Write;

    Some(true)
}

/// Handles the Write state: writes response and manages keep-alive
fn handle_write_state(socket_data: &mut SocketData) -> Option<bool> {

    let response = socket_data.status.response.as_mut()?;

    // Write the response
    let write_result = write_response(&mut socket_data.stream, response);

    match write_result {
        Some(true) => {}       // Write complete, check if finished
        other => return other, // Would block or error
    }

    // Check if response is finished
    if !response.is_finished() {
        return Some(true);
    }


    // Check for keep-alive
    let request = socket_data.status.request.get()?;
    let keep_alive = should_keep_alive(request);

    if keep_alive {
        // Reset for next request
        socket_data.status.status = Status::Read;
        socket_data.status.request = HttpRequestBuilder::new();
        socket_data.status.response = None;
        Some(true)
    } else {
        // Close connection
        println!("Closing connection.");
        let _ = socket_data.stream.shutdown(Shutdown::Both);
        None
    }
}

/// Main handler function

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

        // println!("listener_map: {:#?}", listener_map);
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
        match socket_data.status.status {
            Status::Read => handle_read_state(socket_data, listener_info),
            Status::Write => handle_write_state(socket_data),
            Status::Finish => None,
        }
    }
}
