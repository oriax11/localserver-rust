use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};
use uuid::Uuid;

use crate::request::HttpRequest;
use crate::utils::cookie::Cookie;

#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub created_at: Instant,
    pub expires_at: Instant,
    pub data: HashMap<String, String>,
}

impl Session {
    pub fn new() -> Self {
        let id = Uuid::new_v4().to_string();
        let now = Instant::now();

        Session {
            id,
            created_at: now,
            expires_at: now + Duration::from_secs(3600), 
            data: HashMap::new(),
        }
    }

    pub fn set_expiry(&mut self, duration: Duration) {
        self.expires_at = Instant::now() + duration;
    }

    pub fn set_data(&mut self, key: &str, value: &str) {
        self.data.insert(key.to_string(), value.to_string());
    }

    pub fn get_data(&self, key: &str) -> Option<&String> {
        self.data.get(key)
    }

    pub fn remove_data(&mut self, key: &str) -> Option<String> {
        self.data.remove(key)
    }

    pub fn is_expired(&self) -> bool {
        Instant::now() >= self.expires_at
    }

    pub fn renew(&mut self) {
        self.expires_at = Instant::now() + Duration::from_secs(3600);
    }
}

#[derive(Clone)]
pub struct SessionStore {
    inner: Rc<RefCell<HashMap<String, Session>>>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    /// Create a new anonymous session
    pub fn create(&self) -> Session {
        let session = Session::new();
        self.inner
            .borrow_mut()
            .insert(session.id.clone(), session.clone());
        session
    }

    /// Get a session by ID
    pub fn get(&self, session_id: &str) -> Option<Session> {
        let sessions = self.inner.borrow();
        sessions.get(session_id).cloned()
    }

    /// Update a session
    /// Update a session
    pub fn update(&self, session: &Session) -> bool {
        let mut sessions = self.inner.borrow_mut();

        if sessions.contains_key(&session.id) {
            sessions.insert(session.id.clone(), session.clone());
            true
        } else {
            false
        }
    }

    /// Clean up expired sessions
    pub fn cleanup(&self) -> usize {
        let mut sessions = self.inner.borrow_mut();
        let before = sessions.len();
        sessions.retain(|_, session| !session.is_expired());
        before - sessions.len()
    }

    pub fn with_session<F>(&self, session_id: &str, mut f: F) -> bool
    where
        F: FnMut(&mut Session),
    {
        let mut sessions = self.inner.borrow_mut();
        if let Some(session) = sessions.get_mut(session_id) {
            f(session);
            true
        } else {
            false
        }
    }
}

pub fn handle_session(request: &HttpRequest, session_store: &mut SessionStore) -> Cookie {
    if let Some(session_id) = &request.session_id {
        // Existing session: increment visits and renew expiry
        session_store.with_session(session_id, |session| {
            let visits = session
                .data
                .get("visits")
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(0);
            session
                .data
                .insert("visits".to_string(), (visits + 1).to_string());
            session.renew();
        });

        // Return cookie (refresh max_age)
        Cookie::new("session_id", session_id)
            .path("/")
            .http_only(true)
            .max_age(3600)
    } else {
        // No session: create new
        let mut session = session_store.create();
        session.data.insert("visits".to_string(), "1".to_string());
        let new_session_id = session.id.clone();

        // Save session in store
        session_store.update(&session);

        // Create Set-Cookie header
        Cookie::new("session_id", &new_session_id)
            .path("/")
            .http_only(true)
            .max_age(3600)
    }
}
