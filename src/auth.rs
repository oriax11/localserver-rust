
use crate::request::HttpRequest;
use crate::utils::cookie::Cookie;
use crate::utils::session::SessionStore;
use std::collections::HashMap;
use std::time::Duration;

pub fn handle_login(request: &HttpRequest, session_store: &SessionStore) -> Vec<u8> {
    // Parse form data from POST body
    let body = request.body.as_ref().map(|b| String::from_utf8_lossy(b)).unwrap_or_default();
    let params: HashMap<_, _> = body
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .collect();
    
    let username = params.get("username").map(|s| s.to_string()).unwrap_or_default();
    let password = params.get("password").map(|s| s.to_string()).unwrap_or_default();

    // Simple validation (REPLACE with database in production)
    let valid_users = vec![
        ("admin", "admin123"),
        ("user", "user123"),
        ("test", "test123"),
    ];

    let is_valid = valid_users.iter()
        .any(|(u, p)| u == &username && p == &password);

    if is_valid {
        // Create session
        let session = session_store.create_with_user(&username, &username);
        
        // Build cookie
        let cookie = Cookie::new("session_id", &session.id)
            .path("/")
            .http_only(true)
            .max_age(3600); // 1 hour

        // HTML response
        let html = format!(
            "<!DOCTYPE html>
            <html>
            <head>
                <title>Login Successful</title>
                <meta http-equiv=\"refresh\" content=\"2;url=/dashboard\">
            </head>
            <body>
                <h1>Login Successful!</h1>
                <p>Welcome back, {}! Redirecting...</p>
            </body>
            </html>",
            username
        );

        format!(
            "HTTP/1.1 200 OK\r\n\
             Set-Cookie: {}\r\n\
             Content-Type: text/html\r\n\
             Content-Length: {}\r\n\
             \r\n{}",
            cookie.to_header_value(),
            html.len(),
            html
        ).into_bytes()
    } else {
        let html = "<!DOCTYPE html>
        <html>
        <head><title>Login Failed</title></head>
        <body>
            <h1>Login Failed</h1>
            <p>Invalid credentials</p>
            <a href=\"/login.html\">Try again</a>
        </body>
        </html>";

        format!(
            "HTTP/1.1 401 Unauthorized\r\n\
             Content-Type: text/html\r\n\
             Content-Length: {}\r\n\
             \r\n{}",
            html.len(),
            html
        ).into_bytes()
    }
}

pub fn handle_logout(request: &HttpRequest, session_store: &SessionStore) -> Vec<u8> {
    // Destroy session
    if let Some(session_id) = request.get_session_id() {
        session_store.destroy(session_id);
    }

    let html = "<!DOCTYPE html>
    <html>
    <head>
        <title>Logged Out</title>
        <meta http-equiv=\"refresh\" content=\"2;url=/\">
    </head>
    <body>
        <h1>Logged Out</h1>
        <p>Redirecting...</p>
    </body>
    </html>";

    format!(
        "HTTP/1.1 200 OK\r\n\
         Set-Cookie: {}\r\n\
         Content-Type: text/html\r\n\
         Content-Length: {}\r\n\
         \r\n{}",
        Cookie::delete_cookie("session_id"),
        html.len(),
        html
    ).into_bytes()
}

pub fn handle_dashboard(request: &HttpRequest, session_store: &SessionStore) -> Vec<u8> {
    if let Some(session_id) = request.get_session_id() {
        if let Some(session) = session_store.get(session_id) {
            if session.is_logged_in() {
                let default_username = "User".to_string();
                let username = session.username.as_ref().unwrap_or(&default_username);
                
                let html = format!(
                    "<!DOCTYPE html>
                    <html>
                    <head>
                        <title>Dashboard</title>
                        <style>
                            body {{ font-family: Arial; max-width: 800px; margin: auto; padding: 20px; }}
                            .info {{ background: #f0f0f0; padding: 20px; border-radius: 5px; }}
                        </style>
                    </head>
                    <body>
                        <h1>Dashboard</h1>
                        <div class=\"info\">
                            <h2>Welcome, {}!</h2>
                            <p>You are logged in.</p>
                            <p><strong>Session ID:</strong> {}</p>
                        </div>
                        <br>
                        <a href=\"/profile\">View Profile</a> | 
                        <a href=\"/logout\">Logout</a>
                    </body>
                    </html>",
                    username,
                    session.id
                );

                return format!(
                    "HTTP/1.1 200 OK\r\n\
                     Content-Type: text/html\r\n\
                     Content-Length: {}\r\n\
                     \r\n{}",
                    html.len(),
                    html
                ).into_bytes();
            }
        }
    }

    // Not authenticated - redirect to login
    b"HTTP/1.1 302 Found\r\nLocation: /login.html\r\n\r\n".to_vec()
}

pub fn handle_profile(request: &HttpRequest, session_store: &SessionStore) -> Vec<u8> {
    if let Some(session_id) = request.get_session_id() {
        if let Some(session) = session_store.get(session_id) {
            if session.is_logged_in() {
                let default_username = "User".to_string();
                let username = session.username.as_ref().unwrap_or(&default_username);
                
                let html = format!(
                    "<!DOCTYPE html>
                    <html>
                    <head>
                        <title>Profile</title>
                        <style>
                            body {{ font-family: Arial; max-width: 800px; margin: auto; padding: 20px; }}
                        </style>
                    </head>
                    <body>
                        <h1>Profile: {}</h1>
                        <p><strong>User ID:</strong> {}</p>
                        <p><strong>Logged in since:</strong> {:.2?}</p>
                        <br>
                        <a href=\"/dashboard\">Back to Dashboard</a> | 
                        <a href=\"/logout\">Logout</a>
                    </body>
                    </html>",
                    username,
                    session.user_id.as_ref().unwrap_or(&"N/A".to_string()),
                    session.created_at.elapsed()
                );

                return format!(
                    "HTTP/1.1 200 OK\r\n\
                     Content-Type: text/html\r\n\
                     Content-Length: {}\r\n\
                     \r\n{}",
                    html.len(),
                    html
                ).into_bytes();
            }
        }
    }

    b"HTTP/1.1 302 Found\r\nLocation: /login.html\r\n\r\n".to_vec()
}

// Helper to check if a path requires authentication
pub fn requires_auth(path: &str) -> bool {
    let protected_paths = [
        "/dashboard",
        "/profile",
        "/settings",
        "/logout",
    ];

    protected_paths.iter().any(|p| path.starts_with(p))
}