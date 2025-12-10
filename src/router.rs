use crate::request::HttpRequest;

type Handler = fn(&HttpRequest) -> Vec<u8>; // renvoie directement les bytes

pub struct Router {
    routes: std::collections::HashMap<String, Handler>,
}

impl Router {
    pub fn new() -> Self {
        Router {
            routes: std::collections::HashMap::new(),
        }
    }

    pub fn handle(&mut self, path: &str, handler: Handler) {
        self.routes.insert(path.to_string(), handler);
    }

    pub fn route(&self, path: &str) -> Option<Handler> {
        self.routes.get(path).copied()
    }
}
