
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone)]
pub struct Cookie {
    name: String,
    value: String,
    path: Option<String>,
    domain: Option<String>,
    max_age: Option<Duration>,
    expires: Option<SystemTime>,
    secure: bool,
    http_only: bool,
    same_site: Option<SameSite>,
}

#[derive(Debug, Clone)]
pub enum SameSite {
    Strict,
    Lax,
    None,
}

impl Cookie {
    /// Create a new cookie
    pub fn new(name: &str, value: &str) -> Self {
        Cookie {
            name: name.to_string(),
            value: value.to_string(),
            path: None,
            domain: None,
            max_age: None,
            expires: None,
            secure: false,
            http_only: false,
            same_site: None,
        }
    }

    /// Set the cookie path
    pub fn path(mut self, path: &str) -> Self {
        self.path = Some(path.to_string());
        self
    }

    /// Set the cookie domain
    pub fn domain(mut self, domain: &str) -> Self {
        self.domain = Some(domain.to_string());
        self
    }

    /// Set the cookie max age in seconds
    pub fn max_age(mut self, seconds: u64) -> Self {
        self.max_age = Some(Duration::from_secs(seconds));
        self
    }

    /// Set the cookie expiry time
    pub fn expires(mut self, time: SystemTime) -> Self {
        self.expires = Some(time);
        self
    }

    /// Mark cookie as secure (HTTPS only)
    pub fn secure(mut self, secure: bool) -> Self {
        self.secure = secure;
        self
    }

    /// Mark cookie as HTTP only (inaccessible to JavaScript)
    pub fn http_only(mut self, http_only: bool) -> Self {
        self.http_only = http_only;
        self
    }

    /// Set SameSite attribute
    pub fn same_site(mut self, same_site: SameSite) -> Self {
        self.same_site = Some(same_site);
        self
    }

    /// Convert cookie to header value string
    pub fn to_header_value(&self) -> String {
        let mut parts = Vec::new();
        
        // Name=Value
        parts.push(format!("{}={}", self.name, urlencoding::encode(&self.value)));
        
        // Path
        if let Some(path) = &self.path {
            parts.push(format!("Path={}", path));
        }
        
        // Domain
        if let Some(domain) = &self.domain {
            parts.push(format!("Domain={}", domain));
        }
        
        // Max-Age
        if let Some(max_age) = &self.max_age {
            parts.push(format!("Max-Age={}", max_age.as_secs()));
        }
        
        // Expires
        if let Some(expires) = &self.expires {
            let formatted = httpdate::fmt_http_date(*expires);
            parts.push(format!("Expires={}", formatted));
        }
        
        // Secure
        if self.secure {
            parts.push("Secure".to_string());
        }
        
        // HttpOnly
        if self.http_only {
            parts.push("HttpOnly".to_string());
        }
        
        // SameSite
        if let Some(same_site) = &self.same_site {
            let value = match same_site {
                SameSite::Strict => "Strict",
                SameSite::Lax => "Lax",
                SameSite::None => "None",
            };
            parts.push(format!("SameSite={}", value));
        }
        
        parts.join("; ")
    }

    /// Parse a cookie from a Cookie header value
    pub fn parse(cookie_string: &str) -> Vec<Self> {
        cookie_string
            .split(';')
            .filter_map(|part| {
                let part = part.trim();
                if part.is_empty() {
                    return None;
                }
                
                if let Some((name, value)) = part.split_once('=') {
                    Some(Cookie::new(name.trim(), value.trim()))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Create a cookie deletion header (sets max-age to 0)
    pub fn delete_cookie(name: &str) -> String {
        format!("{}=; Max-Age=0; Path=/", name)
    }

    /// Get cookie name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get cookie value
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Check if cookie is expired
    pub fn is_expired(&self) -> bool {
        if let Some(expires) = &self.expires {
            SystemTime::now() >= *expires
        } else if let Some(max_age) = &self.max_age {
            // For simplicity, we assume max_age was set relative to creation time
            // In a real implementation, you'd need to track creation time
            // Note: max_age is just a Duration, not tied to when the cookie was set
            // So we can't accurately determine if it's expired without tracking creation time
            false
        } else {
            false
        }
    }
}

impl std::fmt::Display for Cookie {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_header_value())
    }
}

// Helper function to extract session ID from cookies
pub fn extract_session_id(cookie_header: Option<&str>) -> Option<String> {
    cookie_header.and_then(|header| {
        Cookie::parse(header)
            .into_iter()
            .find(|cookie| cookie.name() == "session_id")
            .map(|cookie| cookie.value().to_string())
    })
}