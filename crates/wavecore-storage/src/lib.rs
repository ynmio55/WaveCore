use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use url::Url;

#[derive(Debug, Clone, PartialEq)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
}

impl Cookie {
    pub fn parse(header: &str, current_url: &str) -> Option<Self> {
        let mut parts = header.split(';');
        let (name, value) = parts.next()?.split_once('=')?;
        let name = name.trim().to_string();
        let value = value.trim().to_string();

        let parsed_url = Url::parse(current_url).ok();
        let default_domain = parsed_url
            .as_ref()
            .and_then(|u| u.host_str())
            .unwrap_or("")
            .to_string();
        let default_path = parsed_url
            .as_ref()
            .map(|u| default_cookie_path(u.path()))
            .unwrap_or_else(|| "/".to_string());

        let mut domain = default_domain;
        let mut path = default_path;
        let mut secure = false;
        let mut http_only = false;

        for part in parts {
            let part = part.trim();
            if let Some((k, v)) = part.split_once('=') {
                match k.trim().to_ascii_lowercase().as_str() {
                    "domain" => domain = v.trim().trim_start_matches('.').to_string(),
                    "path" => path = v.trim().to_string(),
                    _ => {}
                }
            } else {
                match part.to_ascii_lowercase().as_str() {
                    "secure" => secure = true,
                    "httponly" => http_only = true,
                    _ => {}
                }
            }
        }

        Some(Self {
            name,
            value,
            domain,
            path,
            secure,
            http_only,
        })
    }

    pub fn matches_url(&self, target_url: &Url) -> bool {
        if let Some(host) = target_url.host_str() {
            let host = host.to_ascii_lowercase();
            let domain = self.domain.to_ascii_lowercase();
            let domain_matches = host == domain
                || host
                    .strip_suffix(&domain)
                    .map(|prefix| prefix.ends_with('.'))
                    .unwrap_or(false);
            if !domain_matches {
                return false;
            }
        } else {
            return false;
        }

        if !target_url.path().starts_with(&self.path) {
            return false;
        }

        if self.secure && target_url.scheme() != "https" {
            return false;
        }

        true
    }
}

fn default_cookie_path(path: &str) -> String {
    if !path.starts_with('/') || path == "/" {
        return "/".to_string();
    }
    let Some(last_slash) = path.rfind('/') else {
        return "/".to_string();
    };
    if last_slash == 0 {
        "/".to_string()
    } else {
        path[..last_slash].to_string()
    }
}

#[derive(Debug, Default, Clone)]
pub struct CookieJar {
    cookies: Vec<Cookie>,
}

impl CookieJar {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_cookie(&mut self, cookie: Cookie) {
        self.cookies.retain(|c| !(c.name == cookie.name && c.domain == cookie.domain && c.path == cookie.path));
        self.cookies.push(cookie);
    }

    pub fn process_set_cookie_header(&mut self, header_val: &str, current_url: &str) {
        if let Some(c) = Cookie::parse(header_val, current_url) {
            self.add_cookie(c);
        }
    }

    pub fn cookie_header_for_url(&self, url_str: &str) -> Option<String> {
        let parsed = Url::parse(url_str).ok()?;
        let matching: Vec<String> = self
            .cookies
            .iter()
            .filter(|c| c.matches_url(&parsed))
            .map(|c| format!("{}={}", c.name, c.value))
            .collect();

        if matching.is_empty() {
            None
        } else {
            Some(matching.join("; "))
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct WebStorage {
    data: HashMap<String, String>,
    storage_file: Option<PathBuf>,
}

impl WebStorage {
    pub fn new_in_memory() -> Self {
        Self::default()
    }

    pub fn new_persistent(storage_dir: &Path, origin: &str) -> Self {
        let safe_origin = origin.replace(|c: char| !c.is_alphanumeric(), "_");
        let file_path = storage_dir.join(format!("{}.json", safe_origin));
        let mut storage = Self {
            data: HashMap::new(),
            storage_file: Some(file_path.clone()),
        };

        if let Ok(content) = fs::read_to_string(&file_path) {
            storage.load_json(&content);
        }
        storage
    }

    pub fn get_item(&self, key: &str) -> Option<&str> {
        self.data.get(key).map(String::as_str)
    }

    pub fn set_item(&mut self, key: impl Into<String>, val: impl Into<String>) {
        self.data.insert(key.into(), val.into());
        self.save();
    }

    pub fn remove_item(&mut self, key: &str) {
        self.data.remove(key);
        self.save();
    }

    pub fn clear(&mut self) {
        self.data.clear();
        self.save();
    }

    pub fn length(&self) -> usize {
        self.data.len()
    }

    fn save(&self) {
        let Some(path) = &self.storage_file else {
            return;
        };
        if let Some(parent) = path.parent() {
            if fs::create_dir_all(parent).is_err() {
                return;
            }
        }
        let Ok(json) = serde_json::to_string_pretty(&self.data) else {
            return;
        };

        let tmp = path.with_extension("json.tmp");
        if fs::write(&tmp, json).is_err() {
            return;
        }
        if fs::rename(&tmp, path).is_err() {
            let _ = fs::remove_file(path);
            let _ = fs::rename(&tmp, path);
        }
    }

    fn load_json(&mut self, json_str: &str) {
        if let Ok(data) = serde_json::from_str::<HashMap<String, String>>(json_str) {
            self.data = data;
        }
    }
}

#[derive(Debug, Clone)]
pub struct CachedResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub etag: Option<String>,
    pub max_age_seconds: Option<u64>,
    pub created_at_secs: u64,
}

impl CachedResponse {
    pub fn new(
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
        etag: Option<String>,
        max_age_seconds: Option<u64>,
        created_at_secs: u64,
    ) -> Self {
        Self {
            status,
            headers,
            body,
            etag,
            max_age_seconds,
            created_at_secs,
        }
    }

    pub fn is_fresh(&self, now_secs: u64) -> bool {
        if let Some(max_age) = self.max_age_seconds {
            now_secs.saturating_sub(self.created_at_secs) < max_age
        } else {
            false
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct HttpCache {
    entries: HashMap<String, CachedResponse>,
}

impl HttpCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, url: &str) -> Option<&CachedResponse> {
        self.entries.get(url)
    }

    pub fn put(&mut self, url: impl Into<String>, res: CachedResponse) {
        self.entries.insert(url.into(), res);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_parsing_and_matching() {
        let cookie = Cookie::parse(
            "sessionId=abc12345; Domain=example.com; Path=/; Secure; HttpOnly",
            "https://example.com/login",
        )
        .unwrap();

        assert_eq!(cookie.name, "sessionId");
        assert_eq!(cookie.value, "abc12345");
        assert_eq!(cookie.domain, "example.com");
        assert!(cookie.secure);
        assert!(cookie.http_only);

        let url_https = Url::parse("https://example.com/dashboard").unwrap();
        let url_http = Url::parse("http://example.com/dashboard").unwrap();
        let url_other = Url::parse("https://google.com/search").unwrap();

        assert!(cookie.matches_url(&url_https));
        assert!(!cookie.matches_url(&url_http)); // secure prevents http
        assert!(!cookie.matches_url(&url_other));
    }

    #[test]
    fn cookie_jar_flow() {
        let mut jar = CookieJar::new();
        jar.process_set_cookie_header("theme=dark; Path=/", "https://example.com/settings");
        jar.process_set_cookie_header("lang=th; Path=/", "https://example.com/settings");

        let header = jar.cookie_header_for_url("https://example.com/home").unwrap();
        assert!(header.contains("theme=dark"));
        assert!(header.contains("lang=th"));
    }

    #[test]
    fn web_storage_memory_operations() {
        let mut storage = WebStorage::new_in_memory();
        assert_eq!(storage.length(), 0);

        storage.set_item("username", "wavecore_user");
        storage.set_item("theme", "midnight");
        assert_eq!(storage.length(), 2);
        assert_eq!(storage.get_item("username"), Some("wavecore_user"));

        storage.remove_item("theme");
        assert_eq!(storage.length(), 1);
        assert_eq!(storage.get_item("theme"), None);

        storage.clear();
        assert_eq!(storage.length(), 0);
    }
    #[test]
    fn cookie_domain_boundary_and_default_path_are_safe() {
        let cookie = Cookie::parse(
            "session=1; Domain=example.com",
            "https://example.com/account/login",
        )
        .unwrap();
        assert_eq!(cookie.path, "/account");
        assert!(cookie.matches_url(&Url::parse("https://sub.example.com/account/home").unwrap()));
        assert!(!cookie.matches_url(&Url::parse("https://notexample.com/account/home").unwrap()));
    }

    #[test]
    fn persistent_storage_round_trips_special_characters() {
        let dir = std::env::temp_dir().join(format!("wavecore-storage-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        {
            let mut storage = WebStorage::new_persistent(&dir, "https://example.com");
            storage.set_item("quoted:key", "line1\n\"quoted\"\\path");
        }
        let storage = WebStorage::new_persistent(&dir, "https://example.com");
        assert_eq!(
            storage.get_item("quoted:key"),
            Some("line1\n\"quoted\"\\path")
        );
        let _ = fs::remove_dir_all(&dir);
    }

}
