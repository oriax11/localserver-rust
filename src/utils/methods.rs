#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub enum HttpMethod {
    GET,
    POST,
    DELETE,
    Other(String),
}

impl HttpMethod {
    pub fn from_str(method: &str) -> HttpMethod {
        let method = method.to_uppercase();
        match method.as_str() {
            "GET" => HttpMethod::GET,
            "POST" => HttpMethod::POST,
            "DELETE" => HttpMethod::DELETE,
            _ => Self::Other(method.to_string()),
        }
    }
    pub fn to_str(&self) -> &str {
        match self {
            HttpMethod::GET => "GET",
            HttpMethod::POST => "POST",
            HttpMethod::DELETE => "DELETE",
            HttpMethod::Other(method) => method.as_str(),
        }
    }
}
