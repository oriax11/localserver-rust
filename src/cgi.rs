use std::process::{Command, Stdio};
use std::io::Write;
use crate::request::HttpRequest;

pub fn execute_cgi(request: &HttpRequest, root: &str) -> Vec<u8> {
    let script_path = format!("{}{}", root, &request.path);

    let (path_info, query_string) = if let Some(pos) = request.path.find('?') {
        (&request.path[..pos], &request.path[pos+1..])
    } else {
        (&request.path[..], "")
    };

    let mut cmd = Command::new(script_path);
    cmd.env("REQUEST_METHOD", &request.method)
       .env("QUERY_STRING", query_string)
       .env("PATH_INFO", path_info)
       .stdin(Stdio::piped())
       .stdout(Stdio::piped());

    let mut child = cmd.spawn().expect("Failed to execute CGI");

    // Envoyer body si POST
    if request.method.to_uppercase() == "POST" {
        if let Some(body) = &request.body {
            child.stdin.as_mut().unwrap().write_all(body).unwrap();
        }
    }

    let output = child.wait_with_output().expect("Failed to wait CGI");

    // Préparer headers HTTP
    let mut response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n",
        output.stdout.len()
    ).into_bytes();

    response.extend_from_slice(&output.stdout);
    response
}
