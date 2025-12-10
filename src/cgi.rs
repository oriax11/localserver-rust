use crate::request::HttpRequest;

pub fn is_cgi_request(path: &str) -> bool { 
    path.ends_with(".cgi") || path.ends_with(".pl") 
}

pub fn execute_cgi(_request: &HttpRequest) -> Vec<u8> {
    b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nHello from CGI!\n".to_vec()
}
