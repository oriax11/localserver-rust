
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub user_id: Option<String>,
    pub username: Option<String>,
    pub created_at: Instant,
    pub expires_at: Instant,
    pub data: HashMap<String, String>,
}

impl Session {
    pub fn new(session_id: Option<String>) -> Self {
        let id = session_id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let now = Instant::now();
        
        Session {
            id,
            user_id: None,
            username: None,
            created_at: now,
            expires_at: now + Duration::from_secs(3600), // 1 hour default
            data: HashMap::new(),
        }
    }

    pub fn with_user(mut self, user_id: &str, username: &str) -> Self {
        self.user_id = Some(user_id.to_string());
        self.username = Some(username.to_string());
        self
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

    pub fn is_logged_in(&self) -> bool {
        self.user_id.is_some() && !self.is_expired()
    }

    pub fn renew(&mut self) {
        self.expires_at = Instant::now() + Duration::from_secs(3600);
    }
}

#[derive(Debug)]
pub struct SessionStore {
    sessions: Arc<RwLock<HashMap<String, Session>>>,
    //rwlock read write lock to allow many readers or one writer at a time
    //arc Atomic Reference Counting to allow safe sharing across threads
    cleanup_interval: Duration,
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new(Duration::from_secs(300)) // Cleanup every 5 minutes
    }
}

impl SessionStore {
    pub fn new(cleanup_interval: Duration) -> Self {
        let store = SessionStore {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            cleanup_interval,
        };
        
        // Start cleanup task
        store.start_cleanup();
        
        store
    }

    /// Create a new session for a user
    pub fn create_with_user(&self, user_id: &str, username: &str) -> Session {
        let session = Session::new(None).with_user(user_id, username);
        self.sessions.write()
            .expect("Failed to acquire write lock on sessions")
            .insert(session.id.clone(), session.clone());
        session
    }

    /// Create a new anonymous session
    pub fn create(&self) -> Session {
        let session = Session::new(None);
        self.sessions.write()
            .expect("Failed to acquire write lock on sessions")
            .insert(session.id.clone(), session.clone());
        session
    }

    /// Get a session by ID
    pub fn get(&self, session_id: &str) -> Option<Session> {
        let sessions = self.sessions.read()
            .expect("Failed to acquire read lock on sessions");
        
        sessions.get(session_id).cloned()
    }

    /// Update a session
    pub fn update(&self, session: &Session) -> bool {
        let mut sessions = self.sessions.write()
            .expect("Failed to acquire write lock on sessions");
        
        if sessions.contains_key(&session.id) {
            sessions.insert(session.id.clone(), session.clone());
            true
        } else {
            false
        }
    }

    /// Destroy a session by ID
    pub fn destroy(&self, session_id: &str) -> bool {
        self.sessions.write()
            .expect("Failed to acquire write lock on sessions")
            .remove(session_id)
            .is_some()
    }

    /// Clean up expired sessions
    pub fn cleanup(&self) -> usize {
        let mut sessions = self.sessions.write()
            .expect("Failed to acquire write lock on sessions");
        
        let before = sessions.len();
        sessions.retain(|_, session| !session.is_expired());
        let after = sessions.len();
        
        before - after
    }

    /// Get all active sessions
    pub fn get_all(&self) -> Vec<Session> {
        let sessions = self.sessions.read()
            .expect("Failed to acquire read lock on sessions");
        
        sessions.values().cloned().collect()
    }

    /// Start background cleanup task
    fn start_cleanup(&self) {
        let store = self.clone();
        
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(store.cleanup_interval);
                let removed = store.cleanup();
                if removed > 0 {
                    println!("Session cleanup: removed {} expired sessions", removed);
                }
            }
        });
    }
}

impl Clone for SessionStore {
    fn clone(&self) -> Self {
        SessionStore {
            sessions: Arc::clone(&self.sessions),
            cleanup_interval: self.cleanup_interval,
        }
    }
}