use std::fs;
use std::path::Path;
use url::Url;

#[derive(Debug, Clone, PartialEq)]
pub struct ResourceResponse {
    pub url: String,
    pub content: String,
    pub content_type: String,
}

#[derive(Debug)]
pub enum NetError {
    Network(String),
    Io(std::io::Error),
    InvalidUrl(String),
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetError::Network(s) => write!(f, "Network error: {s}"),
            NetError::Io(e) => write!(f, "IO error: {e}"),
            NetError::InvalidUrl(s) => write!(f, "Invalid URL: {s}"),
        }
    }
}

impl std::error::Error for NetError {}

pub fn fetch_resource(url_or_path: &str) -> Result<ResourceResponse, NetError> {
    let trimmed = url_or_path.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        let resp = ureq::get(trimmed)
            .set("User-Agent", "WaveCore/0.1 (Experimental Browser Engine; Rust)")
            .call()
            .map_err(|e| NetError::Network(e.to_string()))?;

        let content_type = resp.content_type().to_string();
        let content = resp
            .into_string()
            .map_err(|e| NetError::Network(format!("Failed to read response body: {e}")))?;

        Ok(ResourceResponse {
            url: trimmed.to_string(),
            content,
            content_type,
        })
    } else {
        let clean_path = trimmed.strip_prefix("file://").unwrap_or(trimmed);
        let path = Path::new(clean_path);
        let content = fs::read_to_string(path).map_err(NetError::Io)?;
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let content_type = match ext {
            "html" | "htm" => "text/html",
            "css" => "text/css",
            _ => "text/plain",
        }
        .to_string();

        Ok(ResourceResponse {
            url: trimmed.to_string(),
            content,
            content_type,
        })
    }
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
        if target.starts_with("http://") || target.starts_with("https://") || target.starts_with("file://") {
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

        assert_eq!(nav.go_back(), Some("https://example.com/page1"));
        assert_eq!(nav.current_url(), Some("https://example.com/page1"));
        assert!(nav.can_go_forward());

        assert_eq!(nav.go_forward(), Some("https://example.com/page2"));
    }

    #[test]
    fn relative_url_resolution() {
        let nav = NavigationController::new("https://example.com/blog/post1".to_string());
        assert_eq!(nav.resolve_relative("post2"), "https://example.com/blog/post2");
        assert_eq!(nav.resolve_relative("/about"), "https://example.com/about");
        assert_eq!(nav.resolve_relative("https://rust-lang.org"), "https://rust-lang.org");
    }
}
