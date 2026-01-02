use crate::{
    config::Route,
    request::HttpRequest,
    response::HttpResponseBuilder,
    server::{SimpleResponse, SocketData, Status},
};
use std::{process::{Command, Stdio}, ptr::NonNull};
use std::io::Write;

/// Structure pour les données CGI (sans référence à socket_data)
pub struct CgiContext {
    pub method: String,
    pub path: String,
    pub query_string: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl CgiContext {
    /// Extrait les données nécessaires de la request
    pub fn from_request(request: &HttpRequest) -> Self {
        let (path_only, query_string) = match request.path.split_once('?') {
            Some((p, q)) => (p.to_string(), q.to_string()),
            None => (request.path.clone(), String::new()),
        };

        let headers: Vec<(String, String)> = request
            .headers
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        Self {
            method: request.method.to_str().to_string(),
            path: path_only,
            query_string,
            headers,
            body: request.body.clone().unwrap_or(Vec::new()),
        }
    }
}

pub fn run_cgi(
    route: &Route,
    context: CgiContext,
    script_path: &str,
    socket_data: &mut SocketData,
) -> bool {
    // Déterminer l'interpréteur basé sur l'extension
    let interpreter = match route.cgi.as_deref() {
        Some(".py") => "python3",
        Some(".php") => "php",
        Some(".sh") => "bash",
        Some(".pl") => "perl",
        _ => {
            eprintln!("Unsupported CGI extension: {:?}", route.cgi);
            send_error_response(socket_data, 500, "Unsupported CGI extension");
            return false;
        }
    };

    println!("Executing CGI: {} {} with query: {}", interpreter, script_path, context.query_string);

    // Construire la commande
    let mut cmd = Command::new(interpreter);
    cmd.arg(script_path)
        .env("REQUEST_METHOD", &context.method)
        .env("QUERY_STRING", &context.query_string)
        .env("SCRIPT_FILENAME", script_path)
        .env("PATH_INFO", &context.path)
        .env("SERVER_PROTOCOL", "HTTP/1.1")
        .env("GATEWAY_INTERFACE", "CGI/1.1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Ajouter les headers HTTP comme variables d'environnement CGI
    for (key, value) in &context.headers {
        let env_key = format!("HTTP_{}", key.to_uppercase().replace("-", "_"));
        cmd.env(env_key, value);
    }

    // Si c'est un POST avec un body, configurer stdin
    if context.method == "POST" && !context.body.is_empty() {
        cmd.stdin(Stdio::piped());
        cmd.env("CONTENT_LENGTH", context.body.len().to_string());
        
        // Détecter le Content-Type
        if let Some((_, content_type)) = context.headers.iter().find(|(k, _)| k.to_lowercase() == "content-type") {
            cmd.env("CONTENT_TYPE", content_type);
        }
    }

    // Spawner le processus
    match cmd.spawn() {
        Ok(mut child) => {
            // Si POST, écrire le body dans stdin
            if context.method == "POST" && !context.body.is_empty() {
                if let Some(mut stdin) = child.stdin.take() {
                    if let Err(e) = stdin.write_all(&context.body) {
                        eprintln!("Failed to write body to CGI stdin: {:?}", e);
                        send_error_response(socket_data, 500, "Failed to send data to CGI script");
                        return false;
                    }
                    // Fermer stdin pour signaler la fin du body
                    drop(stdin);
                }
            }

            // Attendre la fin du processus
            match child.wait_with_output() {
                Ok(output) => {
                    if output.status.success() {
                        println!("CGI execution successful");
                        
                        // Construire la réponse HTTP
                        let response = HttpResponseBuilder::new(200, "OK")
                            .header("Content-Type", "text/html")
                            .body(output.stdout)
                            .build();

                        socket_data.status.response = Some(Box::new(SimpleResponse::new(response)));
                        socket_data.status.status = Status::Write;
                        true
                    } else {
                        eprintln!("CGI script failed with status: {:?}", output.status);
                        eprintln!("stderr: {}", String::from_utf8_lossy(&output.stderr));
                        
                        send_error_response(socket_data, 500, "CGI script execution failed");
                        true // On retourne true car on a géré l'erreur
                    }
                }
                Err(e) => {
                    eprintln!("Failed to wait for CGI process: {:?}", e);
                    send_error_response(socket_data, 500, "CGI process error");
                    false
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to spawn CGI process: {:?}", e);
            send_error_response(socket_data, 500, "Failed to start CGI script");
            false
        }
    }
}

/// Helper pour envoyer une réponse d'erreur
fn send_error_response(socket_data: &mut SocketData, status_code: u16, message: &str) {
    let error_body = format!("<html><body><h1>{} Error</h1><p>{}</p></body></html>", status_code, message);
    
    let response = HttpResponseBuilder::new(status_code, message)
        .header("Content-Type", "text/html")
        .body(error_body.into_bytes())
        .build();

    socket_data.status.response = Some(Box::new(SimpleResponse::new(response)));
    socket_data.status.status = Status::Write;
}