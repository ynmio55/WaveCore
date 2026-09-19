use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::Path;
use url::Url;
use wavecore_storage::{CachedResponse, CookieJar, HttpCache};

#[derive(Debug, Clone, PartialEq)]
pub struct HttpResponse {
    pub url: String,
    pub status_code: u16,
    pub status_text: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub content_type: String,
    pub content: String,
}

impl HttpResponse {
    pub fn new(
        url: String,
        status_code: u16,
        status_text: String,
        headers: HashMap<String, String>,
        body: Vec<u8>,
        content_type: String,
    ) -> Self {
        let content = String::from_utf8_lossy(&body).to_string();
        Self {
            url,
            status_code,
            status_text,
            headers,
            body,
            content_type,
            content,
        }
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    pub fn is_ok(&self) -> bool {
        (200..=299).contains(&self.status_code)
    }
}

pub type ResourceResponse = HttpResponse;

#[derive(Debug)]
pub enum NetError {
    Network(String),
    Io(std::io::Error),
    InvalidUrl(String),
    TooManyRedirects(usize),
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetError::Network(s) => write!(f, "Network error: {s}"),
            NetError::Io(e) => write!(f, "IO error: {e}"),
            NetError::InvalidUrl(s) => write!(f, "Invalid URL: {s}"),
            NetError::TooManyRedirects(n) => write!(f, "Exceeded maximum redirect limit ({n})"),
        }
    }
}

impl std::error::Error for NetError {}

pub struct NetworkClient {
    pub cookie_jar: CookieJar,
    pub cache: HttpCache,
    pub user_agent: String,
    pub max_redirects: usize,
}

impl Default for NetworkClient {
    fn default() -> Self {
        Self {
            cookie_jar: CookieJar::new(),
            cache: HttpCache::new(),
            user_agent: "WaveCore/0.2 (Production Engine Prototype; Linux/x86_64)".to_string(),
            max_redirects: 10,
        }
    }
}

impl NetworkClient {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn fetch(&mut self, url_or_path: &str) -> Result<HttpResponse, NetError> {
        let trimmed = url_or_path.trim();

        // 1. Data URIs (RFC 2397)
        if trimmed.starts_with("data:") {
            let rest = &trimmed[5..];
            let (metadata, data) = if let Some(comma_pos) = rest.find(',') {
                (&rest[..comma_pos], &rest[comma_pos + 1..])
            } else {
                ("text/plain", rest)
            };
            let content_type = if metadata.is_empty() {
                "text/plain".to_string()
            } else {
                metadata.split(';').next().unwrap_or("text/plain").to_string()
            };
            let body_bytes = data.as_bytes().to_vec();
            return Ok(HttpResponse::new(
                trimmed.to_string(),
                200,
                "OK".to_string(),
                HashMap::new(),
                body_bytes,
                content_type,
            ));
        }

        // 2. Local File / file:// URIs
        if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
            let clean_path = trimmed.strip_prefix("file://").unwrap_or(trimmed);
            let path = Path::new(clean_path);
            let body_bytes = fs::read(path).map_err(NetError::Io)?;
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let content_type = match ext {
                "html" | "htm" => "text/html",
                "css" => "text/css",
                "png" => "image/png",
                "jpg" | "jpeg" => "image/jpeg",
                "json" => "application/json",
                _ => "text/plain",
            }
            .to_string();

            return Ok(HttpResponse::new(
                trimmed.to_string(),
                200,
                "OK".to_string(),
                HashMap::new(),
                body_bytes,
                content_type,
            ));
        }

        // 3. HTTP / HTTPS with Redirects, Cache, and Cookies
        let mut current_url = trimmed.to_string();
        let mut redirect_count = 0;

        loop {
            if redirect_count >= self.max_redirects {
                return Err(NetError::TooManyRedirects(self.max_redirects));
            }

            // Check Cache
            let cached_etag = self.cache.get(&current_url).and_then(|c| c.etag.clone());

            let mut req = ureq::get(&current_url)
                .set("User-Agent", &self.user_agent)
                .set("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8");

            // Attach Cookies
            if let Some(cookie_str) = self.cookie_jar.cookie_header_for_url(&current_url) {
                req = req.set("Cookie", &cookie_str);
            }

            // Attach Cache Validation
            if let Some(ref etag) = cached_etag {
                req = req.set("If-None-Match", etag);
            }

            let response = match req.call() {
                Ok(resp) => resp,
                Err(ureq::Error::Status(304, resp)) => {
                    // 304 Not Modified -> serve from cache!
                    if let Some(cached) = self.cache.get(&current_url) {
                        let mut headers_map = HashMap::new();
                        for (k, v) in &cached.headers {
                            headers_map.insert(k.clone(), v.clone());
                        }
                        let content_type = headers_map
                            .get("content-type")
                            .cloned()
                            .unwrap_or_else(|| "text/html".to_string());
                        return Ok(HttpResponse::new(
                            current_url,
                            200,
                            "OK (Cached 304)".to_string(),
                            headers_map,
                            cached.body.clone(),
                            content_type,
                        ));
                    }
                    resp
                }
                Err(ureq::Error::Status(code, resp)) => {
                    // Handle 3xx redirects manually if needed
                    if (300..=399).contains(&code) {
                        if let Some(loc) = resp.header("Location") {
                            let next_url = if let Ok(base_parsed) = Url::parse(&current_url) {
                                base_parsed.join(loc).map(|u| u.to_string()).unwrap_or_else(|_| loc.to_string())
                            } else {
                                loc.to_string()
                            };
                            current_url = next_url;
                            redirect_count += 1;
                            continue;
                        }
                    }
                    resp
                }
                Err(e) => return Err(NetError::Network(e.to_string())),
            };

            let status_code = response.status();
            let status_text = response.status_text().to_string();

            // Extract Headers
            let mut headers_map = HashMap::new();
            for header_name in response.headers_names() {
                if let Some(val) = response.header(&header_name) {
                    let name_lower = header_name.to_ascii_lowercase();
                    // Process Set-Cookie
                    if name_lower == "set-cookie" {
                        self.cookie_jar.process_set_cookie_header(val, &current_url);
                    }
                    headers_map.insert(name_lower, val.to_string());
                }
            }

            // Handle 301, 302, 307, 308 redirects
            if (300..=399).contains(&status_code) {
                if let Some(loc) = headers_map.get("location") {
                    let next_url = if let Ok(base_parsed) = Url::parse(&current_url) {
                        base_parsed.join(loc).map(|u| u.to_string()).unwrap_or_else(|_| loc.clone())
                    } else {
                        loc.clone()
                    };
                    current_url = next_url;
                    redirect_count += 1;
                    continue;
                }
            }

            let content_type = response.content_type().to_string();

            // Stream / read response body into bytes
            let mut reader = response.into_reader();
            let mut body_bytes = Vec::new();
            reader.read_to_end(&mut body_bytes).map_err(NetError::Io)?;

            // Store in Cache if ETag present
            let etag = headers_map.get("etag").cloned();
            let header_list: Vec<(String, String)> = headers_map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            self.cache.put(
                &current_url,
                CachedResponse {
                    status: status_code,
                    headers: header_list,
                    body: body_bytes.clone(),
                    etag,
                },
            );

            return Ok(HttpResponse::new(
                current_url,
                status_code,
                status_text,
                headers_map,
                body_bytes,
                content_type,
            ));
        }
    }
}

// Global convenience function
pub fn fetch_resource(url_or_path: &str) -> Result<HttpResponse, NetError> {
    let mut client = NetworkClient::new();
    client.fetch(url_or_path)
}

#[derive(Debug, Clone)]
pub struct NavigationController {
    pub history: Vec<String>,
    pub current_index: usize,
}

impl NavigationController {
    pub fn new(initial_url: String) -> Self {
        Self {
            history: vec![initial_url],
            current_index: 0,
        }
    }

    pub fn current_url(&self) -> Option<&str> {
        self.history.get(self.current_index).map(String::as_str)
    }

    pub fn push(&mut self, url: String) {
        if self.current_url() == Some(&url) {
            return;
        }
        if self.current_index + 1 < self.history.len() {
            self.history.truncate(self.current_index + 1);
        }
        self.history.push(url);
        self.current_index = self.history.len() - 1;
    }

    pub fn can_go_back(&self) -> bool {
        self.current_index > 0
    }

    pub fn go_back(&mut self) -> Option<&str> {
        if self.can_go_back() {
            self.current_index -= 1;
            self.current_url()
        } else {
            None
        }
    }

    pub fn can_go_forward(&self) -> bool {
        self.current_index + 1 < self.history.len()
    }

    pub fn go_forward(&mut self) -> Option<&str> {
        if self.can_go_forward() {
            self.current_index += 1;
            self.current_url()
        } else {
            None
        }
    }

    pub fn resolve_relative(&self, target: &str) -> String {
        let target = target.trim();
        if target.starts_with("http://")
            || target.starts_with("https://")
            || target.starts_with("file://")
            || target.starts_with("data:")
        {
            return target.to_string();
        }

        let Some(base) = self.current_url() else {
            return target.to_string();
        };

        if base.starts_with("http://") || base.starts_with("https://") {
            if let Ok(base_url) = Url::parse(base) {
                if let Ok(joined) = base_url.join(target) {
                    return joined.to_string();
                }
            }
        }

        let base_path = Path::new(base.strip_prefix("file://").unwrap_or(base));
        let parent = base_path.parent().unwrap_or_else(|| Path::new("."));
        let joined = parent.join(target);
        joined.to_string_lossy().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_history_stack() {
        let mut nav = NavigationController::new("https://example.com/page1".to_string());
        assert_eq!(nav.current_url(), Some("https://example.com/page1"));
        assert!(!nav.can_go_back());
        assert!(!nav.can_go_forward());

        nav.push("https://example.com/page2".to_string());
        assert_eq!(nav.current_url(), Some("https://example.com/page2"));
        assert!(nav.can_go_back());

        nav.go_back();
        assert_eq!(nav.current_url(), Some("https://example.com/page1"));
        assert!(nav.can_go_forward());

        nav.go_forward();
        assert_eq!(nav.current_url(), Some("https://example.com/page2"));
    }

    #[test]
    fn relative_url_resolution() {
        let nav = NavigationController::new("https://example.com/articles/intro.html".to_string());
        assert_eq!(
            nav.resolve_relative("chapter1.html"),
            "https://example.com/articles/chapter1.html"
        );
        assert_eq!(
            nav.resolve_relative("/about"),
            "https://example.com/about"
        );
        assert_eq!(
            nav.resolve_relative("https://rust-lang.org"),
            "https://rust-lang.org"
        );
    }

    #[test]
    fn network_client_data_uri_and_headers() {
        let mut client = NetworkClient::new();
        let res = client
            .fetch("data:text/html;charset=utf-8,<h1>WaveCore Production</h1>")
            .unwrap();

        assert_eq!(res.status_code, 200);
        assert_eq!(res.status_text, "OK");
        assert_eq!(res.content_type, "text/html");
        assert_eq!(res.text(), "<h1>WaveCore Production</h1>");
    }
}
