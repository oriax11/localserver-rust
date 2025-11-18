use std::collections::HashMap;

#[derive(Debug, Default, Clone)]
pub struct HttpHeaders {
    inner: HashMap<String, String>,
}

impl HttpHeaders {
    pub fn new() -> Self {
        HttpHeaders {
            inner: HashMap::new(),
        }
    }

    pub fn insert(&mut self, key: &str, value: &str) {
        self.inner
            .insert(key.to_ascii_lowercase(), value.to_string());
    }

    pub fn get(&self, key: &str) -> Option<&String> {
        self.inner.get(&key.to_ascii_lowercase())
    }
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn remove(&mut self, key: &str) -> Option<String> {
        self.inner.remove(&key.to_ascii_lowercase())
    }
}
