use std::collections::HashMap;

use crate::request::HttpRequest;
use crate::response::HttpResponse;

type Handler = fn(&HttpRequest) -> HttpResponse;

pub struct Router {
    routes: HashMap<String, Handler>,
}

impl Router {
    pub fn new() -> Self {
        Router {
            routes: HashMap::new(),
        }
    }

    pub fn handle(&mut self, path: &str, handler: Handler) {
        self.routes.insert(path.to_string(), handler);
    }

    pub fn route(&self, path: &str) -> Option<Handler> {
        self.routes.get(path).copied()
    }
}