use crate::config::Config;
use crate::request::HttpRequestBuilder;
use crate::utils::HttpHeaders;
use crate::utils::HttpMethod;
use mio::net::{TcpListener, TcpStream};
use mio::{Events, Interest, Poll, Token};
use std::collections::HashMap;
use std::io::{self, Read};
use std::time::Instant;

const SERVER_TOKEN: Token = Token(0);

#[derive(PartialEq, Debug)]
enum Status {
    Read,
    Write,
    Finish,
}
#[derive(Debug)]

/// Represents the state of a connection's lifecycle.
struct SocketStatus {
    ttl: Instant,
    status: Status,
    // response: Box<dyn HttpResponseCommon>,
    request: HttpRequestBuilder,
    index_writed: usize,
}

/// Wraps a TCP stream and its associated state.
///
///
///
///
///
#[derive(Debug)]
struct SocketData {
    stream: TcpStream,
    status: Option<SocketStatus>,
}

pub struct Server {
    poll: Poll,
    events: Events,
    listeners: HashMap<Token, TcpListener>,
    connections: HashMap<Token, SocketData>,
    next_token: usize,
}

#[derive(Debug)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub path: String,
    pub args: HashMap<String, String>,
    pub headers: HttpHeaders,
    pub body: Vec<u8>,
    stream: Option<TcpStream>,
    pub buffer: Vec<u8>,
}

impl Server {
    pub fn new() -> io::Result<Self> {
        Ok(Server {
            poll: Poll::new()?,
            events: Events::with_capacity(1024),
            listeners: HashMap::new(),
            connections: HashMap::new(),
            next_token: 1, // Start tokens for connections from 1
        })
    }

    pub fn run(&mut self, config: Config) -> io::Result<()> {
        // For now, let's use the first server config and first port
        let server_config = &config.servers[0];
        let addr = format!("{}:{}", server_config.host, server_config.ports[0])
            .parse()
            .unwrap();

        let mut main_listener = TcpListener::bind(addr)?;
        self.poll
            .registry()
            .register(&mut main_listener, SERVER_TOKEN, Interest::READABLE)?;
        self.listeners.insert(SERVER_TOKEN, main_listener);

        println!("Server listening on {}", addr);

        loop {
            self.poll.poll(&mut self.events, None)?;

            for event in self.events.iter() {
                match event.token() {
                    SERVER_TOKEN => {
                        // Accept new connections
                        loop {
                            match self.listeners.get_mut(&SERVER_TOKEN).unwrap().accept() {
                                Ok((mut stream, _)) => {
                                    let token = Token(self.next_token);
                                    self.next_token += 1;

                                    self.poll.registry().register(
                                        &mut stream,
                                        token,
                                        Interest::READABLE,
                                    )?;

                                    let socket_status = SocketStatus {
                                        ttl: Instant::now(),
                                        status: Status::Read,
                                        request: HttpRequestBuilder::new(),
                                        index_writed: 0,
                                    };
                                    let socket_data = SocketData {
                                        stream,
                                        status: Some(socket_status),
                                    };

                                    self.connections.insert(token, socket_data);
                                    println!("Accepted new connection with token: {:?}", token);
                                }
                                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                                    // No more connections to accept
                                    break;
                                }
                                Err(e) => {
                                    eprintln!("Failed to accept connection: {}", e);
                                    break;
                                }
                            }
                        }
                    }
                    token => {
                        if event.is_readable() {
                            if let Some(socket_data) = self.connections.get_mut(&token) {
                                println!("handling");

                                Server::handle(socket_data);
                            }
                        }
                    }
                }
            }
        }
    }

    fn handle(socket_data: &mut SocketData) -> Option<()> {
        println!("hadnling");
        let status = socket_data.status.as_mut()?;

        match status.status {
            Status::Read => {
                println!("statusread");

                while !status.request.done() {
                    println!("not done ");
                    let mut buffer = [0; 2048];
                    match socket_data.stream.read(&mut buffer) {
                        Err(e) => match e.kind() {
                            io::ErrorKind::WouldBlock => return Some(()),
                            io::ErrorKind::ConnectionReset => return None,
                            _ => {
                                eprintln!("Read error: {:?}", e);
                                return None;
                            }
                        },
                        Ok(m) => {
                            if m == 0 {
                                return None;
                            }
                            status.ttl = Instant::now();
                            let _r: Result<(), &'static str> =
                                status.request.append(buffer[..m].to_vec());
                            println!("{:#?} ", status.request);
                            // if r.is_err() {
                            //     // Early return response if not valid request is sended
                            //     let error_msg = r.err().unwrap();
                            //     let response =
                            //         HttpResponse::new(HttpStatus::BadRequest, error_msg, None)
                            //             .to_bytes();
                            //     let _ = socket_data.stream.write(&response);
                            //     let _ = socket_data.stream.flush();
                            //     let _ = socket_data.stream.shutdown(Shutdown::Both);
                            //     return None;
                            // }
                        }
                    }
                }
                // let request = status.request.get()?;
                // let keep_alive = request
                //     .headers
                //     .get("connection")
                //     .map(|v| v.to_lowercase() == "keep-alive")
                //     .unwrap_or(false);

                // let mut response = action(request);
                // if keep_alive {
                //     response
                //         .base()
                //         .headers
                //         .entry("connection")
                //         .or_insert("keep-alive".to_string());
                //     response.base().headers.insert(
                //         "Keep-Alive",
                //         &format!("timeout={}", KEEP_ALIVE_TTL.as_secs()),
                //     );
                // } else {
                //     response.base().headers.insert("Connection", "close");
                // }
                // status.status = Status::Write;
                // status.response = response;
                // status.response.set_stream(&socket_data.stream);
            }
            Status::Write => {
                // loop {
                //     match status.response.peek() {
                //         Ok(n) => match socket_data.stream.write(&n) {
                //             Ok(_) => {
                //                 status.ttl = Instant::now();
                //                 let _ = status.response.next();
                //             }
                //             Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => return Some(()),
                //             Err(e) => {
                //                 eprintln!("Write error: {:?}", e);
                //                 return None;
                //             }
                //         },
                //         Err(IterError::WouldBlock) => {
                //             status.ttl = Instant::now();
                //             return Some(());
                //         }
                //         Err(_) => break,
                //     }
                // }
                // status.status = Status::Finish;
                // let request = status.request.get()?;
                // let keep_alive = request
                //     .headers
                //     .get("connection")
                //     .map(|v| v.to_lowercase() == "keep-alive")
                //     .unwrap_or(false);
                // if keep_alive {
                //     status.status = Status::Read;
                //     status.index_writed = 0;
                //     status.request = HttpRequestBuilder::new();
                //     return Some(());
                // } else {
                //     let _ = socket_data.stream.shutdown(Shutdown::Both);
                //     return None;
                // }
            }
            Status::Finish => {
                return None;
            }
        };
        Some(())

        // If the request is not yet complete, read data from the stream into a buffer.
        // This ensures that the server can handle partial or chunked requests.

        // Seting the stream in case is needed for the response, (example: streaming)
        // Write the response to the client in chunks
    }
}
