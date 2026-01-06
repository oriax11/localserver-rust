use crate::cgi::run_cgi;
use crate::config::{Config, Route, ServerConfig};
use crate::handler::*;
use crate::request::HttpRequest;
use crate::request::HttpRequestBuilder;
use crate::response::{HttpResponseBuilder, detect_content_type, handle_method_not_allowed};
use crate::router::Router;
use crate::utils::cookie::Cookie;
use crate::utils::session::{SessionStore, handle_session};
use crate::utils::{HttpHeaders, HttpMethod};
use mio::net::{TcpListener, TcpStream};
use mio::{Events, Interest, Poll, Token};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, BufReader, Read, Write};
use std::net::Shutdown;
use std::time::Instant;

const LISTENER_TOKEN_START: usize = 0;
const CONNECTION_TOKEN_START: usize = 10000;

pub trait HttpResponseCommon {
    fn peek(&self) -> &[u8];
    fn next(&mut self, n: usize);
    fn is_finished(&self) -> bool;
    fn fill_if_needed(&mut self) -> io::Result<()>;
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
    fn fill_if_needed(&mut self) -> io::Result<()> {
        Ok(())
    } // no-op
}

pub struct FileResponse {
    headers: Vec<u8>,
    headers_index: usize,
    headers_sent: bool,
    reader: BufReader<File>,
    buffer: [u8; 8192],
    buf_len: usize,
    buf_index: usize,
    finished: bool,
}

impl FileResponse {
    pub fn new(file_path: &str) -> io::Result<Self> {
        let content_type = detect_content_type(file_path);
        let file = File::open(file_path)?;
        let metadata = file.metadata()?;
        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: {}\r\n\r\n",
            metadata.len(),
            content_type
        )
        .into_bytes();

        Ok(Self {
            headers,
            headers_sent: false,
            headers_index: 0,
            reader: BufReader::new(file),
            buffer: [0; 8192],
            buf_len: 0,
            buf_index: 0,
            finished: false,
        })
    }

    /// Fill the buffer if it's empty
    fn fill_buffer(&mut self) -> io::Result<()> {
        if self.buf_index >= self.buf_len && !self.finished {
            let n = self.reader.read(&mut self.buffer)?;
            self.buf_index = 0;
            self.buf_len = n;
            if n == 0 {
                self.finished = true;
            }
        }
        Ok(())
    }
}

impl HttpResponseCommon for FileResponse {
    fn peek(&self) -> &[u8] {
        if !self.headers_sent {
            &self.headers[self.headers_index..]
        } else {
            &self.buffer[self.buf_index..self.buf_len]
        }
    }

    fn next(&mut self, n: usize) {
        if !self.headers_sent {
            self.headers_index += n;
            if self.headers_index >= self.headers.len() {
                self.headers_sent = true;
            }
        } else {
            self.buf_index += n;
        }
    }

    fn is_finished(&self) -> bool {
        self.headers_sent && self.finished && self.buf_index >= self.buf_len
    }

    fn fill_if_needed(&mut self) -> io::Result<()> {
        if self.headers_sent && self.buf_index >= self.buf_len && !self.finished {
            self.fill_buffer()?;
        }
        Ok(())
    }
}

#[derive(PartialEq, Debug)]
pub enum Status {
    Read,
    Write,
    Finish,
}

pub struct SocketStatus {
    pub ttl: Instant,
    pub status: Status,
    pub request: HttpRequestBuilder,
    pub response: Option<Box<dyn HttpResponseCommon>>,
}

pub struct SocketData {
    pub stream: TcpStream,
    pub status: SocketStatus,
    pub listener_token: Token,
    pub session_store: SessionStore, // Session store for authentication
}

pub struct ListenerInfo {
    listener: TcpListener,
    host: String,
    port: u16,
    servers: Vec<ServerConfig>,
    default_server_index: usize,
}

pub struct Server {
    poll: Poll,
    events: Events,
    listeners: HashMap<Token, ListenerInfo>,
    connections: HashMap<Token, SocketData>,
    router: Router,
    session_store: SessionStore, // Session store for authentication
    next_token: usize,
}

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

fn extract_hostname(headers: &HttpHeaders) -> &str {
    headers
        .get("host")
        .and_then(|h| h.split(':').next())
        .unwrap_or("")
}

fn select_server<'a>(listener_info: &'a ListenerInfo, hostname: &str) -> &'a ServerConfig {
    if let Some(srv) = listener_info
        .servers
        .iter()
        .find(|s| s.server_name == hostname)
    {
        println!(
            "Selected server '{}' for Host: {}",
            srv.server_name, hostname
        );
        return srv;
    }

    let default_index = listener_info.default_server_index;
    let default_srv = listener_info.servers.get(default_index).unwrap_or_else(|| {
        panic!(
            "Invalid default_server_index {} for listener with {} servers",
            default_index,
            listener_info.servers.len()
        )
    });

    println!(
        "No match for Host: '{}', using default server '{}'",
        hostname, default_srv.server_name
    );

    default_srv
}

fn get_error_page_path(server: &ServerConfig, status_code: u16) -> String {
    server
        .error_pages
        .iter()
        .find(|ep| ep.code == status_code)
        .map(|ep| ep.path.clone())
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

use std::path::Path;

fn resolve_file_path(
    server: &ServerConfig,
    route: &crate::config::Route,
    request_path: &str,
) -> Option<String> {
    println!(
        "Resolving file path for request_path: '{}' under route: '{}'",
        request_path, route.path
    );
    let server_root = &server.root;
    let route_root = &route.root;
    let base = format!("{}/{}", server_root, route_root);

    let base_path = match Path::new(&base).canonicalize() {
        Ok(path) => path,
        Err(_) => return None,
    };

    let relative_path = request_path
        .strip_prefix(&route.path)
        .unwrap_or("")
        .trim_start_matches('/');

    let full_path = base_path.join(relative_path);
    let canonical = match full_path.canonicalize() {
        Ok(path) => path,
        Err(_) => {
            let parent = full_path.parent()?;
            let canonical_parent = parent.canonicalize().ok()?;
            if !canonical_parent.starts_with(&base_path) {
                return None;
            }
            full_path
        }
    };

    if canonical.starts_with(&base_path) {
        canonical.to_str().map(|s| s.to_string())
    } else {
        None
    }
}

fn read_request(stream: &mut TcpStream, request: &mut HttpRequestBuilder) -> Option<bool> {
    let mut buf = [0u8; 2048];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => {
                return None;
            }
            Ok(n) => {
                if let Err(_e) = request.append(buf[..n].to_vec()) {
                    return None;
                }
                if request.done() {
                    return Some(true);
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                return Some(false);
            }
            Err(_) => {
                return None;
            }
        }
    }
}

fn write_response(
    stream: &mut TcpStream,
    response: &mut Box<dyn HttpResponseCommon>,
) -> Option<bool> {
    response.fill_if_needed().ok()?;

    let data = response.peek();

    if data.is_empty() {
        return Some(true);
    }
    match stream.write(data) {
        Ok(n) => {
            response.next(n);
            if response.is_finished() {
                Some(false)
            } else {
                Some(true)
            }
        }
        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Some(false),
        Err(_) => None,
    }
}

fn should_keep_alive(request: &crate::request::HttpRequest) -> bool {
    request
        .headers
        .get("connection")
        .map(|v| v.to_lowercase() == "keep-alive")
        .unwrap_or(false)
}

fn handle_read_state(
    socket_data: &mut SocketData,
    listener_info: Option<&ListenerInfo>,
) -> Option<bool> {
    let read_result = read_request(&mut socket_data.stream, &mut socket_data.status.request);

    match read_result {
        Some(true) => {}
        other => return other,
    }

    let request: &HttpRequest = socket_data.status.request.get()?;

    // handle cookies and sessions
    let cookie: Cookie = handle_session(request, &mut socket_data.session_store);

    // Select server based on Host header
    let hostname = extract_hostname(&request.headers);
    let info = listener_info.expect("No listener info available");
    let selected_server: &ServerConfig = select_server(info, hostname);


    let selected_route = find_matching_route(selected_server, &request.path);

    if let Some(route) = selected_route {
        if let Some(redirect) = &route.redirect {
            let response_bytes = HttpResponseBuilder::redirect(redirect).cookie(&cookie).build();
            socket_data.status.response = Some(Box::new(SimpleResponse::new(response_bytes)));
        } else {
            let request_method = &request.method;
            let method_allowed = route
                .methods
                .iter()
                .any(|m| HttpMethod::from_str(m) == *request_method);

            if !method_allowed {
                let allowed = &route.methods;
                let response_bytes = handle_method_not_allowed(&allowed, &selected_server , &cookie);
                socket_data.status.response = Some(Box::new(SimpleResponse::new(response_bytes)));
            } else {
                let file_path = resolve_file_path(selected_server, route, &request.path)
                    .unwrap_or_else(|| "".to_string());

                if let Some(cgi_ext) = &route.cgi {
                    if request.path.ends_with(cgi_ext) {
                        let cgi_context = crate::cgi::CgiContext::from_request(request);
                        if run_cgi(route, cgi_context, &file_path, socket_data) {
                            return Some(true);
                        } else {
                            return None;
                        }
                    }
                }

                let response: Box<dyn HttpResponseCommon> = match request_method {
                    HttpMethod::GET => {
                        handle_get(&file_path, &selected_server, &request, &cookie)
                    }
                    HttpMethod::POST => {
                        let response_bytes = handle_post(&file_path, &request , &cookie);
                        Box::new(SimpleResponse::new(response_bytes))
                    }
                    HttpMethod::DELETE => {
                        let error_path = get_error_page_path(selected_server, 404);
                        let response_bytes = handle_delete(&file_path, &error_path , &cookie);
                        Box::new(SimpleResponse::new(response_bytes))
                    }
                    HttpMethod::Other(_) => {
                        let allowed = &route.methods;
                        let response_bytes = handle_method_not_allowed(&allowed, &selected_server , &cookie);
                        Box::new(SimpleResponse::new(response_bytes))
                    }
                };

                socket_data.status.response = Some(response);
            }
        }
    } else {
        let error_path = get_error_page_path(selected_server, 404);
        let response_bytes = HttpResponseBuilder::serve_error_page(&error_path, 404, "Not Found" , &cookie);
        socket_data.status.response = Some(Box::new(SimpleResponse::new(response_bytes)));
    }

    socket_data.status.status = Status::Write;
    Some(true)
}

fn handle_write_state(socket_data: &mut SocketData) -> Option<bool> {
    let response = socket_data.status.response.as_mut()?;

    let write_result = write_response(&mut socket_data.stream, response);

    match write_result {
        Some(true) => {}
        other => {
            return other;
        }
    }

    if !response.is_finished() {
        println!("Response not finished yet.");
        return Some(true);
    }

    let request = socket_data.status.request.get()?;
    let keep_alive = should_keep_alive(request);

    if keep_alive {
        socket_data.status.status = Status::Read;
        socket_data.status.request = HttpRequestBuilder::new();
        socket_data.status.response = None;
        println!("Keeping connection alive for next request.");
        Some(true)
    } else {
        println!("Closing connection.");
        let _ = socket_data.stream.shutdown(Shutdown::Both);
        None
    }
}

impl Server {
    pub fn new() -> io::Result<Self> {
        Ok(Server {
            poll: Poll::new()?,
            events: Events::with_capacity(1024),
            listeners: HashMap::new(),
            connections: HashMap::new(),
            router: Router::new(),
            session_store: SessionStore::new(),
            next_token: CONNECTION_TOKEN_START,
        })
    }

    pub fn run(&mut self, config: Config) -> io::Result<()> {
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

                if token.0 < CONNECTION_TOKEN_START {
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
                                            listener_token: token,
                                            session_store: self.session_store.clone(),
                                        },
                                    );

                                    println!(
                                        "Accepted connection {:?} from listener {:?}",
                                        conn_token, token
                                    );
                                }
                                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                                    break;
                                }
                                Err(e) => {
                                    eprintln!("Accept error: {:?}", e);
                                    break;
                                }
                            }
                        }
                    }
                } else {
                    if let Some(socket_data) = self.connections.get_mut(&token) {
                        loop {
                            let listener_info = self.listeners.get(&socket_data.listener_token);
                            match Server::handle(socket_data, listener_info) {
                                Some(true) => {
                                    continue;
                                }
                                Some(false) => {
                                    break;
                                }
                                None => {
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
