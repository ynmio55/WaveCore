use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use url::Url;
use wavecore_layout::Rect;
use wavecore_render::DisplayCommand;

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
                if let Some(scheme_source) = val.strip_suffix(':') {
                    if !scheme_source.contains('/') && scheme_source == target_url_parsed.scheme() {
                        return true;
                    }
                }
                if let Some(domain) = val.strip_prefix("*.") {
                    let target_host = target_host.to_ascii_lowercase();
                    let domain = domain.to_ascii_lowercase();
                    if target_host
                        .strip_suffix(&domain)
                        .map(|prefix| prefix.ends_with('.') && !prefix.is_empty())
                        .unwrap_or(false)
                    {
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

pub fn validate_brokered_resource_url(url: &str) -> Result<Url, String> {
    let parsed = Url::parse(url).map_err(|e| format!("SecurityError: invalid resource URL: {e}"))?;
    match parsed.scheme() {
        "http" | "https" => {
            if parsed.host_str().is_none() {
                return Err("SecurityError: network resource URL has no host".to_string());
            }
            Ok(parsed)
        }
        "data" => Ok(parsed),
        scheme => Err(format!(
            "SecurityError: renderer resource scheme '{scheme}' is not allowed through the network broker"
        )),
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

    pub fn check_direct_network_access(&self) -> Result<(), String> {
        if !self.can_access_network_directly {
            Err(format!(
                "SecurityError: Render process {} for origin {:?} must broker network access through the browser process",
                self.process_id, self.origin
            ))
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BrowserToRenderMessage {
    Navigate { url: String, html: String },
    InputClick { x: f32, y: f32 },
    InputChar(char),
    InputBackspace,
    ResourceData { request_id: u64, result: Result<Vec<u8>, String> },
    PermissionResult { permission: Permission, state: PermissionState },
    Shutdown,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RenderToBrowserMessage {
    FetchResource { request_id: u64, url: String, origin: Origin },
    FrameRendered { display_list: Vec<DisplayCommand>, damage_rects: Vec<Rect>, title: Option<String> },
    SetCookie { url: String, cookie_str: String },
    RequestPermission { origin: Origin, permission: Permission },
    ConsoleLog { level: String, message: String },
}

pub struct BrowserEndpoint {
    pub sender: Sender<BrowserToRenderMessage>,
    pub receiver: Receiver<RenderToBrowserMessage>,
}

pub struct RenderEndpoint {
    pub sender: Sender<RenderToBrowserMessage>,
    pub receiver: Receiver<BrowserToRenderMessage>,
}

impl BrowserEndpoint {
    pub fn send(&self, msg: BrowserToRenderMessage) -> Result<(), String> {
        self.sender.send(msg).map_err(|e| e.to_string())
    }

    pub fn try_recv(&self) -> Result<RenderToBrowserMessage, TryRecvError> {
        self.receiver.try_recv()
    }
}

impl RenderEndpoint {
    pub fn send(&self, msg: RenderToBrowserMessage) -> Result<(), String> {
        self.sender.send(msg).map_err(|e| e.to_string())
    }

    pub fn try_recv(&self) -> Result<BrowserToRenderMessage, TryRecvError> {
        self.receiver.try_recv()
    }
}

pub struct IpcChannel;

impl IpcChannel {
    pub fn create_pair() -> (BrowserEndpoint, RenderEndpoint) {
        let (b_tx, r_rx) = mpsc::channel();
        let (r_tx, b_rx) = mpsc::channel();
        (
            BrowserEndpoint { sender: b_tx, receiver: b_rx },
            RenderEndpoint { sender: r_tx, receiver: r_rx },
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessStatus {
    Running,
    Crashed,
    Terminated,
}

pub struct IsolatedRenderHost {
    pub sandbox: RenderProcessSandbox,
    pub endpoint: RenderEndpoint,
    pub status: ProcessStatus,
}

impl IsolatedRenderHost {
    pub fn new(process_id: u32, origin: Origin, endpoint: RenderEndpoint) -> Self {
        Self {
            sandbox: RenderProcessSandbox::new_isolated(process_id, origin),
            endpoint,
            status: ProcessStatus::Running,
        }
    }

    pub fn is_crashed(&self) -> bool {
        self.status == ProcessStatus::Crashed
    }

    pub fn simulate_crash(&mut self) {
        self.status = ProcessStatus::Crashed;
    }

    pub fn respawn(&mut self, new_endpoint: RenderEndpoint) {
        self.status = ProcessStatus::Running;
        self.endpoint = new_endpoint;
    }

    pub fn generate_sad_tab_html() -> &'static str {
        "<!DOCTYPE html><html><head><title>Aw, Snap! - WaveCore</title><style>body{background:#0F172A;color:#F8FAFC;font-family:sans-serif;display:flex;align-items:center;justify-content:center;height:100vh;margin:0;}.crash-box{text-align:center;max-width:480px;padding:32px;background:#1E293B;border-radius:12px;border:1px solid:#334155;}h1{color:#38BDF8;font-size:24px;margin-bottom:8px;}p{color:#94A3B8;font-size:14px;line-height:1.5;}.btn{display:inline-block;margin-top:16px;padding:10px 24px;background:#3B82F6;color:white;border-radius:6px;text-decoration:none;font-weight:bold;}</style></head><body><div class=\"crash-box\"><h1>Aw, Snap!</h1><p>Something went wrong while displaying this webpage.<br>WaveCore protected your system by isolating the crashed render process.</p><p><small style=\"color:#64748B;\">Error code: RESULT_CODE_KILLED_BAD_MESSAGE</small></p><a href=\"#\" class=\"btn\">Reload Page</a></div></body></html>"
    }

    pub fn request_resource(&self, request_id: u64, url: &str) -> Result<(), String> {
        if self.status != ProcessStatus::Running {
            return Err("Render process is not running".to_string());
        }
        validate_brokered_resource_url(url)?;
        // A sandboxed renderer must not open the network itself. Resource loads are
        // intentionally brokered to the browser endpoint, which can apply cookies,
        // CORS, cache policy, permissions, and auditing before returning ResourceData.
        self.endpoint.send(RenderToBrowserMessage::FetchResource {
            request_id,
            url: url.to_string(),
            origin: self.sandbox.origin.clone(),
        })
    }

    pub fn dispatch_frame(
        &self,
        display_list: Vec<DisplayCommand>,
        damage_rects: Vec<Rect>,
        title: Option<String>,
    ) -> Result<(), String> {
        if self.status != ProcessStatus::Running {
            return Err("Render process is not running".to_string());
        }
        self.endpoint.send(RenderToBrowserMessage::FrameRendered {
            display_list,
            damage_rects,
            title,
        })
    }
}

/// Computes the registrable domain (eTLD+1) for Site Isolation.
pub fn registrable_domain(host: &str) -> String {
    let host = host.trim().to_ascii_lowercase();
    if host.is_empty() || host == "localhost" || host.parse::<std::net::IpAddr>().is_ok() {
        return host;
    }
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() <= 2 {
        host
    } else {
        // e.g. ["sub", "example", "com"] -> "example.com"
        // e.g. ["a", "b", "co", "uk"] -> "b.co.uk"
        let len = parts.len();
        let second_last = parts[len - 2];
        if matches!(second_last, "co" | "com" | "net" | "org" | "gov" | "ac" | "in" | "or") && len >= 3 {
            format!("{}.{}.{}", parts[len - 3], parts[len - 2], parts[len - 1])
        } else {
            format!("{}.{}", parts[len - 2], parts[len - 1])
        }
    }
}

/// Represents an isolated site boundary (Scheme + Registrable Domain).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SiteInstance {
    pub site: String,
}

impl SiteInstance {
    pub fn for_origin(origin: &Origin) -> Self {
        match origin {
            Origin::Tuple { scheme, host, .. } => {
                let reg = registrable_domain(host);
                Self {
                    site: format!("{scheme}://{reg}"),
                }
            }
            Origin::Opaque(id) => Self {
                site: format!("opaque:{id}"),
            },
        }
    }
}

/// Manages multi-process Site Isolation, mapping each SiteInstance to a distinct isolated render process.
pub struct SiteIsolationManager {
    instances: HashMap<SiteInstance, u32>,
    next_process_id: u32,
}

impl SiteIsolationManager {
    pub fn new() -> Self {
        Self {
            instances: HashMap::new(),
            next_process_id: 1,
        }
    }

    pub fn get_or_assign_process_id(&mut self, origin: &Origin) -> u32 {
        let site_instance = SiteInstance::for_origin(origin);
        if let Some(&pid) = self.instances.get(&site_instance) {
            pid
        } else {
            let pid = self.next_process_id;
            self.next_process_id += 1;
            self.instances.insert(site_instance, pid);
            pid
        }
    }

    pub fn are_sites_isolated(&self, o1: &Origin, o2: &Origin) -> bool {
        let s1 = SiteInstance::for_origin(o1);
        let s2 = SiteInstance::for_origin(o2);
        s1 != s2
    }
}

impl Default for SiteIsolationManager {
    fn default() -> Self {
        Self::new()
    }
}

/// W3C HTML5 `<iframe>` Sandbox Policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IframeSandboxPolicy {
    pub is_sandboxed: bool,
    pub allow_scripts: bool,
    pub allow_same_origin: bool,
    pub allow_forms: bool,
    pub allow_top_navigation: bool,
}

impl IframeSandboxPolicy {
    pub fn parse(sandbox_attr: Option<&str>) -> Self {
        match sandbox_attr {
            None => Self {
                is_sandboxed: false,
                allow_scripts: true,
                allow_same_origin: true,
                allow_forms: true,
                allow_top_navigation: true,
            },
            Some(tokens) => {
                let parts: Vec<&str> = tokens.split_whitespace().collect();
                Self {
                    is_sandboxed: true,
                    allow_scripts: parts.contains(&"allow-scripts"),
                    allow_same_origin: parts.contains(&"allow-same-origin"),
                    allow_forms: parts.contains(&"allow-forms"),
                    allow_top_navigation: parts.contains(&"allow-top-navigation"),
                }
            }
        }
    }

    pub fn effective_origin(&self, parent_origin: &Origin) -> Origin {
        if !self.is_sandboxed || self.allow_same_origin {
            parent_origin.clone()
        } else {
            Origin::Opaque(format!("sandboxed-iframe-{}", fast_hash(&parent_origin.to_string_repr())))
        }
    }

    pub fn allows_scripts(&self) -> bool {
        !self.is_sandboxed || self.allow_scripts
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
        assert!(sandbox.check_direct_network_access().is_err());
    }

    #[test]
    fn sandboxed_renderer_brokers_resource_requests() {
        let (browser, renderer) = IpcChannel::create_pair();
        let origin = Origin::parse("https://wavecore.dev").unwrap();
        let host = IsolatedRenderHost::new(7, origin.clone(), renderer);

        host.request_resource(99, "https://cdn.wavecore.dev/app.js").unwrap();

        match browser.try_recv().unwrap() {
            RenderToBrowserMessage::FetchResource { request_id, url, origin: msg_origin } => {
                assert_eq!(request_id, 99);
                assert_eq!(url, "https://cdn.wavecore.dev/app.js");
                assert_eq!(msg_origin, origin);
            }
            other => panic!("Unexpected message: {other:?}"),
        }
    }

    #[test]
    fn ipc_channel_bidirectional_flow() {
        let (browser, renderer) = IpcChannel::create_pair();

        // Browser sends Navigate to Renderer
        browser.send(BrowserToRenderMessage::Navigate {
            url: "https://wavecore.dev".to_string(),
            html: "<h1>Hello WaveCore</h1>".to_string(),
        }).unwrap();

        let recv_on_render = renderer.try_recv().unwrap();
        match recv_on_render {
            BrowserToRenderMessage::Navigate { url, html } => {
                assert_eq!(url, "https://wavecore.dev");
                assert_eq!(html, "<h1>Hello WaveCore</h1>");
            }
            _ => panic!("Unexpected message"),
        }

        // Renderer sends FrameRendered to Browser
        renderer.send(RenderToBrowserMessage::FrameRendered {
            display_list: vec![],
            damage_rects: vec![Rect { x: 0.0, y: 0.0, width: 800.0, height: 600.0 }],
            title: Some("WaveCore Dev".to_string()),
        }).unwrap();

        let recv_on_browser = browser.try_recv().unwrap();
        match recv_on_browser {
            RenderToBrowserMessage::FrameRendered { damage_rects, title, .. } => {
                assert_eq!(damage_rects.len(), 1);
                assert_eq!(title.as_deref(), Some("WaveCore Dev"));
            }
            _ => panic!("Unexpected message"),
        }
    }

    #[test]
    fn site_isolation_and_process_mapping() {
        let mut sim = SiteIsolationManager::new();

        let o_mail = Origin::parse("https://mail.google.com").unwrap();
        let o_drive = Origin::parse("https://drive.google.com").unwrap();
        let o_github = Origin::parse("https://github.com").unwrap();

        let pid_mail = sim.get_or_assign_process_id(&o_mail);
        let pid_drive = sim.get_or_assign_process_id(&o_drive);
        let pid_github = sim.get_or_assign_process_id(&o_github);

        // Same site (google.com) gets same process
        assert_eq!(pid_mail, pid_drive);
        // Different site (github.com) gets isolated process
        assert_ne!(pid_mail, pid_github);
        assert!(sim.are_sites_isolated(&o_mail, &o_github));
        assert!(!sim.are_sites_isolated(&o_mail, &o_drive));
    }

    #[test]
    fn iframe_sandbox_policy_enforcement() {
        let parent_origin = Origin::parse("https://trusted.com").unwrap();

        // 1. Unsandboxed iframe
        let unsandboxed = IframeSandboxPolicy::parse(None);
        assert!(!unsandboxed.is_sandboxed);
        assert!(unsandboxed.allows_scripts());
        assert_eq!(unsandboxed.effective_origin(&parent_origin), parent_origin);

        // 2. Sandboxed iframe without allow-scripts or allow-same-origin
        let strict_sandbox = IframeSandboxPolicy::parse(Some(""));
        assert!(strict_sandbox.is_sandboxed);
        assert!(!strict_sandbox.allows_scripts());
        let eff = strict_sandbox.effective_origin(&parent_origin);
        assert!(eff.is_opaque());
        assert!(!eff.is_same_origin(&parent_origin));

        // 3. Sandboxed iframe with allow-scripts but no allow-same-origin
        let script_only = IframeSandboxPolicy::parse(Some("allow-scripts"));
        assert!(script_only.allows_scripts());
        assert!(script_only.effective_origin(&parent_origin).is_opaque());
    }

    #[test]
    fn render_host_crash_recovery_and_respawn() {
        let (_b_ep, r_ep) = IpcChannel::create_pair();
        let origin = Origin::parse("https://crash-test.com").unwrap();
        let mut host = IsolatedRenderHost::new(101, origin.clone(), r_ep);

        assert!(!host.is_crashed());
        assert_eq!(host.status, ProcessStatus::Running);

        // Simulate crash
        host.simulate_crash();
        assert!(host.is_crashed());
        assert_eq!(host.status, ProcessStatus::Crashed);

        // Attempting to send resource request while crashed fails
        assert!(host.request_resource(1, "https://crash-test.com/data").is_err());

        // Check Sad Tab crash screen
        let sad_tab = IsolatedRenderHost::generate_sad_tab_html();
        assert!(sad_tab.contains("Aw, Snap!"));
        assert!(sad_tab.contains("RESULT_CODE_KILLED_BAD_MESSAGE"));

        // Respawn host
        let (_b_ep2, r_ep2) = IpcChannel::create_pair();
        host.respawn(r_ep2);
        assert!(!host.is_crashed());
        assert_eq!(host.status, ProcessStatus::Running);
    }
    #[test]
    fn csp_wildcards_require_real_subdomains_and_support_scheme_sources() {
        let origin = Origin::parse("https://app.example.com").unwrap();
        let csp = ContentSecurityPolicy::parse(
            "script-src https: *.example.com",
        );
        assert!(csp.allows_script(
            &origin,
            Some("https://cdn.example.com/app.js"),
            false
        ));
        assert!(csp.allows_script(
            &origin,
            Some("https://other.net/app.js"),
            false
        ));
        assert!(!csp.allows_script(
            &origin,
            Some("http://evil-example.com/app.js"),
            false
        ));

        let wildcard_only = ContentSecurityPolicy::parse("script-src *.example.com");
        assert!(!wildcard_only.allows_script(
            &origin,
            Some("https://evil-example.com/app.js"),
            false
        ));
        assert!(wildcard_only.allows_script(
            &origin,
            Some("https://static.example.com/app.js"),
            false
        ));
    }

    #[test]
    fn renderer_broker_rejects_privileged_resource_schemes() {
        let (_browser, renderer) = IpcChannel::create_pair();
        let origin = Origin::parse("https://wavecore.dev").unwrap();
        let host = IsolatedRenderHost::new(9, origin, renderer);

        assert!(host.request_resource(1, "file:///etc/passwd").is_err());
        assert!(host.request_resource(2, "javascript:alert(1)").is_err());
        assert!(host.request_resource(3, "https://cdn.wavecore.dev/app.js").is_ok());
        assert!(host.request_resource(4, "data:text/plain,hello").is_ok());
    }

}
