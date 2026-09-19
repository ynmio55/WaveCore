use std::collections::HashMap;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Origin {
    Tuple {
        scheme: String,
        host: String,
        port: u16,
    },
    Opaque(String),
}

impl Origin {
    pub fn parse(input: &str) -> Result<Self, String> {
        let trimmed = input.trim();
        if trimmed.starts_with("data:") || trimmed.starts_with("javascript:") || trimmed.is_empty() {
            return Ok(Origin::Opaque(format!("opaque:{}", fast_hash(trimmed))));
        }

        if trimmed.starts_with("file:") {
            return Ok(Origin::Tuple {
                scheme: "file".to_string(),
                host: String::new(),
                port: 0,
            });
        }

        let parsed = Url::parse(trimmed).map_err(|e| e.to_string())?;
        let scheme = parsed.scheme().to_ascii_lowercase();
        let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
        let port = parsed.port_or_known_default().unwrap_or(match scheme.as_str() {
            "http" => 80,
            "https" => 443,
            _ => 0,
        });

        Ok(Origin::Tuple { scheme, host, port })
    }

    pub fn is_same_origin(&self, other: &Origin) -> bool {
        match (self, other) {
            (
                Origin::Tuple {
                    scheme: s1,
                    host: h1,
                    port: p1,
                },
                Origin::Tuple {
                    scheme: s2,
                    host: h2,
                    port: p2,
                },
            ) => s1 == s2 && h1 == h2 && p1 == p2,
            _ => false, // Opaque origins are never same-origin with anything
        }
    }

    pub fn is_opaque(&self) -> bool {
        matches!(self, Origin::Opaque(_))
    }

    pub fn to_string_repr(&self) -> String {
        match self {
            Origin::Tuple { scheme, host, port } => {
                if *scheme == "file" {
                    "file://".to_string()
                } else if (*scheme == "http" && *port == 80) || (*scheme == "https" && *port == 443) {
                    format!("{scheme}://{host}")
                } else {
                    format!("{scheme}://{host}:{port}")
                }
            }
            Origin::Opaque(id) => id.clone(),
        }
    }
}

fn fast_hash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h = (h ^ (b as u64)).wrapping_mul(0x100000001b3);
    }
    h
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CspDirective {
    pub name: String,
    pub values: Vec<String>,
}

impl CspDirective {
    pub fn allows(&self, target_url: &str, origin: &Origin, is_inline: bool) -> bool {
        if self.values.iter().any(|v| v == "'none'") {
            return false;
        }

        if is_inline {
            return self.values.iter().any(|v| v == "'unsafe-inline'");
        }

        if self.values.iter().any(|v| v == "*") {
            return true;
        }

        if self.values.iter().any(|v| v == "'self'") {
            if let Ok(target_origin) = Origin::parse(target_url) {
                if origin.is_same_origin(&target_origin) {
                    return true;
                }
            }
        }

        if target_url.starts_with("data:") && self.values.iter().any(|v| v == "data:") {
            return true;
        }

        // Host / scheme check
        if let Ok(target_url_parsed) = Url::parse(target_url) {
            let target_host = target_url_parsed.host_str().unwrap_or("");
            for val in &self.values {
                if val == target_host {
                    return true;
                }
                if let Ok(val_url) = Url::parse(val) {
                    if val_url.scheme() == target_url_parsed.scheme()
                        && val_url.host_str() == target_url_parsed.host_str()
                    {
                        return true;
                    }
                }
                if let Some(domain) = val.strip_prefix("*.") {
                    if target_host.ends_with(domain) {
                        return true;
                    }
                }
            }
        }

        false
    }
}

#[derive(Debug, Clone, Default)]
pub struct ContentSecurityPolicy {
    pub directives: HashMap<String, CspDirective>,
}

impl ContentSecurityPolicy {
    pub fn parse(header_value: &str) -> Self {
        let mut directives = HashMap::new();
        for chunk in header_value.split(';') {
            let trimmed = chunk.trim();
            if trimmed.is_empty() {
                continue;
            }
            let mut parts = trimmed.split_whitespace();
            if let Some(name) = parts.next() {
                let name_lower = name.to_ascii_lowercase();
                let values = parts.map(String::from).collect();
                directives.insert(
                    name_lower.clone(),
                    CspDirective {
                        name: name_lower,
                        values,
                    },
                );
            }
        }
        Self { directives }
    }

    fn check_directive(
        &self,
        specific_name: &str,
        target_url: &str,
        origin: &Origin,
        is_inline: bool,
    ) -> bool {
        if let Some(dir) = self.directives.get(specific_name) {
            return dir.allows(target_url, origin, is_inline);
        }
        if let Some(default_dir) = self.directives.get("default-src") {
            return default_dir.allows(target_url, origin, is_inline);
        }
        true // Unrestricted if neither specific nor default directive is present
    }

    pub fn allows_script(&self, origin: &Origin, src: Option<&str>, is_inline: bool) -> bool {
        self.check_directive("script-src", src.unwrap_or(""), origin, is_inline)
    }

    pub fn allows_style(&self, origin: &Origin, src: Option<&str>, is_inline: bool) -> bool {
        self.check_directive("style-src", src.unwrap_or(""), origin, is_inline)
    }

    pub fn allows_image(&self, origin: &Origin, url: &str) -> bool {
        self.check_directive("img-src", url, origin, false)
    }

    pub fn allows_connect(&self, origin: &Origin, url: &str) -> bool {
        self.check_directive("connect-src", url, origin, false)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Permission {
    Geolocation,
    Notifications,
    Camera,
    Microphone,
    ClipboardRead,
    ClipboardWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionState {
    Granted,
    Denied,
    Prompt,
}

#[derive(Debug, Default)]
pub struct PermissionManager {
    permissions: HashMap<(Origin, Permission), PermissionState>,
}

impl PermissionManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn query(&self, origin: &Origin, perm: Permission) -> PermissionState {
        self.permissions
            .get(&(origin.clone(), perm))
            .copied()
            .unwrap_or(PermissionState::Prompt)
    }

    pub fn set_permission(&mut self, origin: Origin, perm: Permission, state: PermissionState) {
        self.permissions.insert((origin, perm), state);
    }
}

#[derive(Debug, Clone)]
pub struct RenderProcessSandbox {
    pub process_id: u32,
    pub origin: Origin,
    pub can_access_network_directly: bool,
    pub can_access_filesystem_directly: bool,
}

impl RenderProcessSandbox {
    pub fn new_isolated(process_id: u32, origin: Origin) -> Self {
        Self {
            process_id,
            origin,
            can_access_network_directly: false,
            can_access_filesystem_directly: false,
        }
    }

    pub fn check_file_access(&self) -> Result<(), String> {
        if !self.can_access_filesystem_directly {
            Err(format!(
                "SecurityError: Render process {} for origin {:?} is sandboxed from filesystem access",
                self.process_id, self.origin
            ))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_origin_policy_rules() {
        let o1 = Origin::parse("https://example.com/page1").unwrap();
        let o2 = Origin::parse("https://example.com:443/page2").unwrap();
        let o3 = Origin::parse("http://example.com/page1").unwrap();
        let o4 = Origin::parse("https://sub.example.com/page1").unwrap();
        let o_opaque = Origin::parse("data:text/html,test").unwrap();

        assert!(o1.is_same_origin(&o2));
        assert!(!o1.is_same_origin(&o3)); // Protocol mismatch
        assert!(!o1.is_same_origin(&o4)); // Domain mismatch
        assert!(o_opaque.is_opaque());
        assert!(!o1.is_same_origin(&o_opaque));
    }

    #[test]
    fn csp_evaluation() {
        let csp = ContentSecurityPolicy::parse(
            "default-src 'self'; script-src 'self' https://trusted.cdn.com; img-src * data:;",
        );
        let origin = Origin::parse("https://myapp.com").unwrap();

        // Scripts
        assert!(csp.allows_script(&origin, Some("https://myapp.com/app.js"), false));
        assert!(csp.allows_script(&origin, Some("https://trusted.cdn.com/lib.js"), false));
        assert!(!csp.allows_script(&origin, Some("https://evil.com/hack.js"), false));
        assert!(!csp.allows_script(&origin, None, true)); // Inline rejected

        // Images
        assert!(csp.allows_image(&origin, "https://anywhere.com/img.png"));
        assert!(csp.allows_image(&origin, "data:image/png;base64,..."));

        // Connect (falls back to default-src 'self')
        assert!(csp.allows_connect(&origin, "https://myapp.com/api/v1"));
        assert!(!csp.allows_connect(&origin, "https://api.external.com/data"));
    }

    #[test]
    fn permission_manager_flow() {
        let mut mgr = PermissionManager::new();
        let origin = Origin::parse("https://maps.example.com").unwrap();

        assert_eq!(mgr.query(&origin, Permission::Geolocation), PermissionState::Prompt);
        mgr.set_permission(origin.clone(), Permission::Geolocation, PermissionState::Granted);
        assert_eq!(mgr.query(&origin, Permission::Geolocation), PermissionState::Granted);
    }

    #[test]
    fn sandbox_process_isolation() {
        let origin = Origin::parse("https://sandbox.test").unwrap();
        let sandbox = RenderProcessSandbox::new_isolated(42, origin);
        assert!(sandbox.check_file_access().is_err());
    }
}
