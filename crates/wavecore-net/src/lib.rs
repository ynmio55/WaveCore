use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::Path;
use url::Url;
use wavecore_sandbox::Origin;
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

fn current_time_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CacheControl {
    pub max_age: Option<u64>,
    pub no_cache: bool,
    pub no_store: bool,
    pub must_revalidate: bool,
    pub is_public: bool,
    pub is_private: bool,
}

impl CacheControl {
    pub fn parse(header: &str) -> Self {
        let mut cc = Self::default();
        for part in header.split(',') {
            let part = part.trim();
            if let Some((k, v)) = part.split_once('=') {
                if k.trim().eq_ignore_ascii_case("max-age") {
                    cc.max_age = v.trim().parse::<u64>().ok();
                }
            } else if part.eq_ignore_ascii_case("no-cache") {
                cc.no_cache = true;
            } else if part.eq_ignore_ascii_case("no-store") {
                cc.no_store = true;
            } else if part.eq_ignore_ascii_case("must-revalidate") {
                cc.must_revalidate = true;
            } else if part.eq_ignore_ascii_case("public") {
                cc.is_public = true;
            } else if part.eq_ignore_ascii_case("private") {
                cc.is_private = true;
            }
        }
        cc
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CorsPolicy;

impl CorsPolicy {
    pub fn check(
        request_origin: Option<&Origin>,
        target_origin: &Origin,
        response_headers: &HashMap<String, String>,
    ) -> Result<(), String> {
        let Some(req_origin) = request_origin else {
            return Ok(());
        };
        if req_origin.is_same_origin(target_origin) {
            return Ok(());
        }

        let allow_origin = response_headers
            .get("access-control-allow-origin")
            .map(|s| s.trim());

        match allow_origin {
            Some("*") => Ok(()),
            Some(allowed) => {
                let req_str = req_origin.to_string_repr();
                if allowed.eq_ignore_ascii_case(&req_str) {
                    Ok(())
                } else {
                    Err(format!(
                        "CORS error: Access-Control-Allow-Origin '{allowed}' does not match request origin '{req_str}'"
                    ))
                }
            }
            None => Err(format!(
                "CORS error: Missing Access-Control-Allow-Origin header for cross-origin request from {:?}",
                req_origin
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsPolicy {
    Strict,
    Permissive,
}

#[derive(Debug)]
pub enum NetError {
    Network(String),
    Io(std::io::Error),
    InvalidUrl(String),
    TooManyRedirects(usize),
    Cors(String),
    CertificateInvalid(String),
    ResourceLimitExceeded { limit_bytes: usize, actual_bytes: usize },
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetError::Network(s) => write!(f, "Network error: {s}"),
            NetError::Io(e) => write!(f, "IO error: {e}"),
            NetError::InvalidUrl(s) => write!(f, "Invalid URL: {s}"),
            NetError::TooManyRedirects(n) => write!(f, "Exceeded maximum redirect limit ({n})"),
            NetError::Cors(s) => write!(f, "{s}"),
            NetError::CertificateInvalid(s) => write!(f, "SSL/TLS certificate error: {s}"),
            NetError::ResourceLimitExceeded { limit_bytes, actual_bytes } => write!(
                f,
                "Resource exceeds configured limit: {actual_bytes} bytes > {limit_bytes} bytes"
            ),
        }
    }
}

impl std::error::Error for NetError {}

pub fn generate_ssl_error_page(url: &str, reason: &str) -> HttpResponse {
    let body = format!(
        "<!DOCTYPE html><html><head><title>Privacy Error - WaveCore</title><style>body{{background:#0F172A;color:#F8FAFC;font-family:sans-serif;display:flex;align-items:center;justify-content:center;height:100vh;margin:0;}}.box{{max-width:540px;padding:36px;background:#1E293B;border-radius:12px;border:1px solid #EF4444;}}h1{{color:#EF4444;font-size:24px;margin-bottom:12px;}}p{{color:#94A3B8;font-size:14px;line-height:1.6;}}code{{color:#F87171;background:#0F172A;padding:2px 6px;border-radius:4px;}}</style></head><body><div class=\"box\"><h1>Your connection is not private</h1><p>Attackers might be trying to steal your information from <b>{}</b>.<br>Reason: <code>{}</code></p><p>WaveCore prevented access to protect your passwords, cookies, and credit cards.</p></div></body></html>",
        url, reason
    );
    let mut headers = HashMap::new();
    headers.insert("content-type".to_string(), "text/html; charset=utf-8".to_string());
    HttpResponse::new(
        url.to_string(),
        495,
        "SSL Certificate Error".to_string(),
        headers,
        body.into_bytes(),
        "text/html; charset=utf-8".to_string(),
    )
}

pub struct NetworkClient {
    pub cookie_jar: CookieJar,
    pub cache: HttpCache,
    pub user_agent: String,
    pub max_redirects: usize,
    pub tls_policy: TlsPolicy,
    pub max_response_bytes: usize,
}

impl Default for NetworkClient {
    fn default() -> Self {
        Self {
            cookie_jar: CookieJar::new(),
            cache: HttpCache::new(),
            user_agent: "WaveCore/0.2 (Production Engine Prototype; Linux/x86_64)".to_string(),
            max_redirects: 10,
            tls_policy: TlsPolicy::Strict,
            max_response_bytes: 32 * 1024 * 1024,
        }
    }
}

impl NetworkClient {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn fetch(&mut self, url_or_path: &str) -> Result<HttpResponse, NetError> {
        self.fetch_with_origin(url_or_path, None)
    }

    pub fn fetch_with_origin(
        &mut self,
        url_or_path: &str,
        caller_origin: Option<&Origin>,
    ) -> Result<HttpResponse, NetError> {
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
            if body_bytes.len() > self.max_response_bytes {
                return Err(NetError::ResourceLimitExceeded {
                    limit_bytes: self.max_response_bytes,
                    actual_bytes: body_bytes.len(),
                });
            }
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
            if let Ok(meta) = fs::metadata(path) {
                let actual = meta.len() as usize;
                if actual > self.max_response_bytes {
                    return Err(NetError::ResourceLimitExceeded {
                        limit_bytes: self.max_response_bytes,
                        actual_bytes: actual,
                    });
                }
            }
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

        // 3. HTTP / HTTPS with Redirects, Cache, Cookies, and CORS
        let mut current_url = trimmed.to_string();
        let mut redirect_count = 0;

        loop {
            if redirect_count >= self.max_redirects {
                return Err(NetError::TooManyRedirects(self.max_redirects));
            }

            let now = current_time_secs();

            // Check fresh cache before network
            if let Some(cached) = self.cache.get(&current_url) {
                if cached.is_fresh(now) {
                    let mut headers_map = HashMap::new();
                    for (k, v) in &cached.headers {
                        headers_map.insert(k.clone(), v.clone());
                    }
                    if let Ok(target_origin) = Origin::parse(&current_url) {
                        CorsPolicy::check(caller_origin, &target_origin, &headers_map)
                            .map_err(NetError::Cors)?;
                    }
                    let content_type = headers_map
                        .get("content-type")
                        .cloned()
                        .unwrap_or_else(|| "text/html".to_string());
                    return Ok(HttpResponse::new(
                        current_url,
                        cached.status,
                        "OK (Fresh Cache)".to_string(),
                        headers_map,
                        cached.body.clone(),
                        content_type,
                    ));
                }
            }

            let cached_etag = self.cache.get(&current_url).and_then(|c| c.etag.clone());

            let mut req = ureq::get(&current_url)
                .set("User-Agent", &self.user_agent)
                .set("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8");

            // Attach Caller Origin for CORS
            if let Some(req_origin) = caller_origin {
                req = req.set("Origin", &req_origin.to_string_repr());
            }

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
                        if let Ok(target_origin) = Origin::parse(&current_url) {
                            CorsPolicy::check(caller_origin, &target_origin, &headers_map)
                                .map_err(NetError::Cors)?;
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

            // Validate CORS policy for cross-origin request
            if let Ok(target_origin) = Origin::parse(&current_url) {
                CorsPolicy::check(caller_origin, &target_origin, &headers_map)
                    .map_err(NetError::Cors)?;
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
            let mut reader = response
                .into_reader()
                .take(self.max_response_bytes.saturating_add(1) as u64);
            let mut body_bytes = Vec::new();
            reader.read_to_end(&mut body_bytes).map_err(NetError::Io)?;
            if body_bytes.len() > self.max_response_bytes {
                return Err(NetError::ResourceLimitExceeded {
                    limit_bytes: self.max_response_bytes,
                    actual_bytes: body_bytes.len(),
                });
            }

            // Parse Cache-Control
            let cc = headers_map
                .get("cache-control")
                .map(|s| CacheControl::parse(s))
                .unwrap_or_default();

            // Store in Cache if not no-store
            if !cc.no_store {
                let etag = headers_map.get("etag").cloned();
                let header_list: Vec<(String, String)> = headers_map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                self.cache.put(
                    &current_url,
                    CachedResponse::new(
                        status_code,
                        header_list,
                        body_bytes.clone(),
                        etag,
                        cc.max_age,
                        now,
                    ),
                );
            }

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

// Global convenience functions
pub fn fetch_resource(url_or_path: &str) -> Result<HttpResponse, NetError> {
    let mut client = NetworkClient::new();
    client.fetch(url_or_path)
}

pub fn fetch_resource_with_origin(
    url_or_path: &str,
    caller_origin: Option<&Origin>,
) -> Result<HttpResponse, NetError> {
    let mut client = NetworkClient::new();
    client.fetch_with_origin(url_or_path, caller_origin)
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

    #[test]
    fn cache_control_parsing_and_freshness() {
        let cc = CacheControl::parse("public, max-age=3600, must-revalidate");
        assert_eq!(cc.max_age, Some(3600));
        assert!(cc.is_public);
        assert!(cc.must_revalidate);
        assert!(!cc.no_store);

        let cached = CachedResponse::new(200, vec![], vec![1, 2, 3], None, Some(60), 1000);
        assert!(cached.is_fresh(1030));
        assert!(!cached.is_fresh(1070));
    }

    #[test]
    fn cors_policy_evaluation() {
        let origin_a = Origin::parse("https://app.example.com").unwrap();
        let origin_b = Origin::parse("https://api.external.com").unwrap();

        let mut headers = HashMap::new();
        // Missing Access-Control-Allow-Origin
        assert!(CorsPolicy::check(Some(&origin_a), &origin_b, &headers).is_err());

        // Wildcard allowed
        headers.insert("access-control-allow-origin".to_string(), "*".to_string());
        assert!(CorsPolicy::check(Some(&origin_a), &origin_b, &headers).is_ok());

        // Matching specific origin
        headers.insert("access-control-allow-origin".to_string(), "https://app.example.com".to_string());
        assert!(CorsPolicy::check(Some(&origin_a), &origin_b, &headers).is_ok());

        // Mismatched origin
        headers.insert("access-control-allow-origin".to_string(), "https://other.com".to_string());
        assert!(CorsPolicy::check(Some(&origin_a), &origin_b, &headers).is_err());
    }

    #[test]
    fn ssl_error_page_generation() {
        let resp = generate_ssl_error_page("https://untrusted.com", "CERT_COMMON_NAME_INVALID");
        assert_eq!(resp.status_code, 495);
        assert!(resp.text().contains("Your connection is not private"));
        assert!(resp.text().contains("CERT_COMMON_NAME_INVALID"));
    }
    #[test]
    fn response_size_limit_rejects_oversized_data_uri() {
        let mut client = NetworkClient::new();
        client.max_response_bytes = 8;
        let err = client.fetch("data:text/plain,0123456789").unwrap_err();
        assert!(matches!(
            err,
            NetError::ResourceLimitExceeded {
                limit_bytes: 8,
                actual_bytes: 10
            }
        ));
    }

}
