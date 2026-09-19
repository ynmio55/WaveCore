//! # Web Platform Tests (WPT) Compatibility Suite for WaveCore
//!
//! Validates compliance with W3C/WHATWG web specifications:
//! - CSS Box-Sizing Level 3 (content-box vs border-box)
//! - HTML5 Doctype sniffing & Standards Mode vs Quirks Mode
//! - HTML5 Character Entity References (named & numeric)
//! - HTML5 Canvas 2D & WebGL Context Interfaces
//! - HTMLMediaElement State Transitions
//! - W3C Same-Origin Policy & Iframe Sandbox Attributes

use std::cell::RefCell;
use std::rc::Rc;
use wavecore_css::parse as parse_css;
use wavecore_dom::Node;
use wavecore_html::{parse_with_mode, DocumentMode};
use wavecore_js::{dom_bridge::DomBridge, vm::VM, JsValue};
use wavecore_layout::layout;
use wavecore_media::{AudioBuffer, MediaElement, ReadyState};
use wavecore_sandbox::{IframeSandboxPolicy, Origin, SiteInstance, SiteIsolationManager};
use wavecore_style::style_tree;

#[test]
fn wpt_css_box_sizing_compliance() {
    let html = "<div id=\"content_box\">ContentBox</div><div id=\"border_box\">BorderBox</div>";
    let css = r#"
        #content_box {
            width: 200px;
            height: 100px;
            padding: 20px;
            border: 10px solid black;
            box-sizing: content-box;
        }
        #border_box {
            width: 200px;
            height: 100px;
            padding: 20px;
            border: 10px solid black;
            box-sizing: border-box;
        }
    "#;

    let (dom, _) = parse_with_mode(html);
    let stylesheet = parse_css(css);
    let root = layout(&style_tree(&dom, &stylesheet), 800.0);

    let content_box = &root.children[0];
    let border_box = &root.children[1];

    // In content-box:
    // content = 200 x 100
    // total rect = (200 + 40 + 20) x (100 + 40 + 20) = 260 x 160
    assert_eq!(content_box.content.width, 200.0);
    assert_eq!(content_box.content.height, 100.0);
    assert_eq!(content_box.rect.width, 260.0);
    assert_eq!(content_box.rect.height, 160.0);

    // In border-box:
    // total rect width = 200, total rect height = 100
    // content = (200 - 60) x (100 - 60) = 140 x 40
    assert_eq!(border_box.rect.width, 200.0);
    assert_eq!(border_box.rect.height, 100.0);
    assert_eq!(border_box.content.width, 140.0);
    assert_eq!(border_box.content.height, 40.0);
}

#[test]
fn wpt_doctype_sniffing_and_standards_mode() {
    let (std_doc, std_mode) = parse_with_mode("<!DOCTYPE html><html><body>Test</body></html>");
    assert_eq!(std_mode, DocumentMode::Standards);
    assert_eq!(std_doc.children.len(), 1);

    let (quirk_doc, quirk_mode) = parse_with_mode("<html><body>No doctype here</body></html>");
    assert_eq!(quirk_mode, DocumentMode::Quirks);
    assert_eq!(quirk_doc.children.len(), 1);
}

#[test]
fn wpt_html_entities_extended_coverage() {
    let html = "<p>&euro; 50 &times; 2 &plusmn; 5 &hellip; &copy; 2026 &deg;C</p>";
    let (dom, _) = parse_with_mode(html);
    let text_node = &dom.children[0].children[0];
    assert_eq!(text_node.inner_text(), "€ 50 × 2 ± 5 … © 2026 °C");
}

#[test]
fn wpt_canvas_2d_and_webgl_interface() {
    let mut canvas = Node::element("canvas", vec![]);
    if let wavecore_dom::NodeType::Element(e) = &mut canvas.node_type {
        e.set_attribute("id", "c");
        e.set_attribute("width", "400");
        e.set_attribute("height", "300");
    }
    let doc = Node::document(vec![canvas]);
    let root = Rc::new(RefCell::new(doc));
    let bridge = DomBridge::new(root);
    let mut vm = VM::new();
    bridge.attach_to_vm(&mut vm);

    let script = r##"
        let c = document.getElementById("c");
        let ctx = c.getContext("2d");
        ctx.fillStyle = "#00FF00";
        ctx.fillRect(0, 0, 100, 100);

        let gl = c.getContext("webgl");
        let buf = gl.createBuffer();
        let pass = ctx.isCanvas2D && gl.isWebGL && buf._webglBufferId == 1;
        pass;
    "##;

    let tokens = wavecore_js::lexer::Lexer::new(script).tokenize().unwrap();
    let stmts = wavecore_js::parser::Parser::new(tokens).parse().unwrap();
    let chunks = wavecore_js::bytecode::Compiler::new().compile(&stmts).unwrap();
    let res = vm.execute(chunks).unwrap();
    assert_eq!(res, JsValue::Boolean(true));
}

#[test]
fn wpt_media_playback_state_lifecycle() {
    let mut video = MediaElement::new_video("sample.mp4", 640, 360);
    assert_eq!(video.width, 640);
    assert_eq!(video.height, 360);
    assert_eq!(video.ready_state, ReadyState::HaveNothing);
    assert!(video.paused);

    video.play();
    assert!(!video.paused);
    assert_eq!(video.ready_state, ReadyState::HaveEnoughData);

    video.duration = 120.0;
    video.step(15.0);
    assert_eq!(video.current_time, 15.0);

    video.seek(119.5);
    video.step(1.0);
    assert_eq!(video.current_time, 120.0);
    assert!(video.ended);
    assert!(video.paused);

    // Audio buffer synth test
    let audio = AudioBuffer::generate_sine_wave(880.0, 0.5, 44100, 0.9);
    assert_eq!(audio.channels, 2);
    assert_eq!(audio.sample_rate, 44100);
    assert!(audio.duration() >= 0.49 && audio.duration() <= 0.51);
}

#[test]
fn wpt_site_isolation_and_iframe_sandbox() {
    let origin_a = Origin::parse("https://account.bank.com").unwrap();
    let origin_b = Origin::parse("https://ad.tracker.com").unwrap();

    let mut sim = SiteIsolationManager::new();
    let pid_a = sim.get_or_assign_process_id(&origin_a);
    let pid_b = sim.get_or_assign_process_id(&origin_b);
    assert_ne!(pid_a, pid_b);
    assert!(sim.are_sites_isolated(&origin_a, &origin_b));

    // Subdomains share site instance
    let origin_a2 = Origin::parse("https://secure.bank.com").unwrap();
    let pid_a2 = sim.get_or_assign_process_id(&origin_a2);
    assert_eq!(pid_a, pid_a2);
    assert_eq!(SiteInstance::for_origin(&origin_a), SiteInstance::for_origin(&origin_a2));

    // Iframe sandbox restriction
    let sb = IframeSandboxPolicy::parse(Some("allow-forms"));
    assert!(sb.is_sandboxed);
    assert!(!sb.allows_scripts());
    assert!(sb.effective_origin(&origin_a).is_opaque());
}
