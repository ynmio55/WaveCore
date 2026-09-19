//! Cross-subsystem web compatibility regression suite.
//!
//! These tests are intentionally small and deterministic. They are not a replacement
//! for the upstream web-platform-tests project; they protect WaveCore behavior that
//! has already been implemented while the official WPT runner is expanded.

use std::cell::RefCell;
use std::rc::Rc;

use wavecore_css::parse as parse_css;
use wavecore_dom::Node;
use wavecore_html::{parse_with_mode, DocumentMode};
use wavecore_js::{eval_script, DomBridge, JsValue, VM};
use wavecore_layout::layout;
use wavecore_net::{CorsPolicy, NavigationController, NetworkClient};
use wavecore_sandbox::Origin;
use wavecore_storage::WebStorage;
use wavecore_style::style_tree;

#[test]
fn compat_html_css_flex_wrap_and_unicode() {
    let html = "<!DOCTYPE html><div class=\"row\"><div>ไทย A</div><div>ไทย B</div></div>";
    let css = ".row { display:flex; flex-wrap:wrap; width:200px; } .row div { width:150px; height:30px; }";
    let (dom, mode) = parse_with_mode(html);
    assert_eq!(mode, DocumentMode::Standards);

    let sheet = parse_css(css);
    let root = layout(&style_tree(&dom, &sheet), 800.0);
    let row = &root.children[0];
    assert_eq!(row.children.len(), 2);
    assert!(row.children[1].rect.y > row.children[0].rect.y);
}

#[test]
fn compat_navigation_url_resolution() {
    let nav = NavigationController::new("https://example.com/app/pages/home.html".into());
    assert_eq!(
        nav.resolve_relative("../assets/app.js"),
        "https://example.com/app/assets/app.js"
    );
    assert_eq!(
        nav.resolve_relative("/api/v1"),
        "https://example.com/api/v1"
    );
}

#[test]
fn compat_fetch_response_shape_and_status() {
    let root = Rc::new(RefCell::new(Node::document(vec![])));
    let bridge = DomBridge::with_url(root, "https://example.com/");
    let mut vm = VM::new();
    bridge.attach_to_vm(&mut vm);

    let result = eval_script(
        r#"
            let status = 0;
            let ok = false;
            fetch("data:text/plain,hello").then(function(r) {
                status = r.status;
                ok = r.ok;
            });
            status + (ok ? 1 : 0);
        "#,
        &mut vm,
    )
    .unwrap();

    assert_eq!(result, JsValue::Number(201.0));
}

#[test]
fn compat_cors_same_origin_and_cross_origin_rules() {
    use std::collections::HashMap;

    let caller = Origin::parse("https://app.example.com").unwrap();
    let same = Origin::parse("https://app.example.com/data").unwrap();
    let cross = Origin::parse("https://api.example.net/data").unwrap();

    assert!(CorsPolicy::check(Some(&caller), &same, &HashMap::new()).is_ok());

    let mut headers = HashMap::new();
    assert!(CorsPolicy::check(Some(&caller), &cross, &headers).is_err());
    headers.insert(
        "access-control-allow-origin".to_string(),
        "https://app.example.com".to_string(),
    );
    assert!(CorsPolicy::check(Some(&caller), &cross, &headers).is_ok());
}

#[test]
fn compat_storage_persists_and_isolates_origins() {
    let base = std::env::temp_dir().join(format!(
        "wavecore-compat-storage-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&base);

    {
        let mut a = WebStorage::new_persistent(&base, "https://a.example");
        a.set_item("theme", "dark");
    }

    let a2 = WebStorage::new_persistent(&base, "https://a.example");
    let b = WebStorage::new_persistent(&base, "https://b.example");
    assert_eq!(a2.get_item("theme"), Some("dark"));
    assert_eq!(b.get_item("theme"), None);

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn compat_network_resource_limits_are_configurable() {
    let mut client = NetworkClient::new();
    client.max_response_bytes = 5;
    assert!(client.fetch("data:text/plain,12345").is_ok());
    assert!(client.fetch("data:text/plain,123456").is_err());
}
