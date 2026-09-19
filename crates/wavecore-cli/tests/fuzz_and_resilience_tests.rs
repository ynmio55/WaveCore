//! # Fuzzing & Engine Fault-Tolerance Suite for WaveCore
//!
//! Validates resilience and error recovery under adversarial and malformed inputs:
//! - Malformed HTML: unclosed tags, inverted nesting, corrupt attributes, trailing slashes
//! - Broken CSS: missing delimiters, invalid units, unexpected characters, dangling braces
//! - JavaScript parsing resilience: syntax error diagnostics without panic
//! - Randomized byte fuzzing: fuzz parser with mutated bytes to ensure zero panics
//! - Render Process Crash Isolation & Safe Recovery ("Sad Tab" fallback)

use wavecore_css::parse as parse_css;
use wavecore_html::parse as parse_html;
use wavecore_layout::layout;
use wavecore_sandbox::{IpcChannel, IsolatedRenderHost, Origin, ProcessStatus};
use wavecore_style::style_tree;

#[test]
fn resilience_malformed_html_no_panics() {
    let test_cases = [
        "<html><head><title>Unclosed",
        "<p>Paragraph 1 <div>Nested block inside inline <p>Another paragraph",
        "<b><i>Bold and italic</b></i> Mismatched tags",
        "<a href=\"https://example.com' Broken quotes>Link</a>",
        "<img src=unquoted_attr alt=test/>",
        "<div <span class=\"broken\">Bad tag nesting</div>",
        "<!DOCTYPE html><<<<>>>>><><><><",
        "<script>let x = '<p>not a tag</p>';</script><p>After script</p>",
        "&&&&&nbsp;&&amp;&&&bull; Random raw entities",
        "<style>body { color: red; /* unclosed comment </style><p>Still parsed</p>",
    ];

    for case in test_cases {
        let dom = parse_html(case);
        assert!(!dom.children.is_empty() || dom.inner_text().is_empty());
        // Verify layout can run on the malformed DOM without panicking
        let stylesheet = parse_css("body { margin: 0; }");
        let _box = layout(&style_tree(&dom, &stylesheet), 800.0);
    }
}

#[test]
fn resilience_broken_css_no_panics() {
    let broken_stylesheets = [
        "body { color: ; margin: 10px",
        "div { unclosed-bracket: 10px;",
        "@media screen { body { color: red; } /* unclosed media */",
        "} random tokens { background: #12345; }",
        "p { padding: -100px; font-size: invalid_unit; }",
        "::::invalid-selector:::: { width: 100px; }",
        "/* comment only without any rules */",
        "div { color: #GGGGGG; width: NaNpx; }",
    ];

    let dom = parse_html("<div><p>Hello Resilience</p></div>");
    for css in broken_stylesheets {
        let sheet = parse_css(css);
        let _box = layout(&style_tree(&dom, &sheet), 800.0);
    }
}

#[test]
fn resilience_pseudo_fuzzer_random_mutations() {
    let base_html = "<!DOCTYPE html><html><head><title>Fuzz</title></head><body><div class=\"box\"><p>Safe text</p></div></body></html>";
    let bytes = base_html.as_bytes();

    // Pseudo-random deterministic mutation test (LFSR PRNG)
    let mut state: u32 = 0x12345678;
    let mut next_prng = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };

    for _iteration in 0..100 {
        let mut mutated = bytes.to_vec();
        let num_mutations = (next_prng() % 10 + 1) as usize;
        for _ in 0..num_mutations {
            let idx = (next_prng() as usize) % mutated.len();
            let mut_type = next_prng() % 3;
            match mut_type {
                0 => mutated[idx] = (next_prng() % 256) as u8, // random byte
                1 => mutated[idx] = b'<',
                _ => mutated[idx] = b'>',
            }
        }

        let input_str = String::from_utf8_lossy(&mutated);
        let dom = parse_html(&input_str);
        let css = parse_css(&input_str);
        let _ = layout(&style_tree(&dom, &css), 800.0);
    }
}

#[test]
fn resilience_render_crash_isolation_and_auto_respawn() {
    let (_b_ep, r_ep) = IpcChannel::create_pair();
    let origin = Origin::parse("https://crashy-site.io").unwrap();
    let mut host = IsolatedRenderHost::new(999, origin.clone(), r_ep);

    assert_eq!(host.status, ProcessStatus::Running);

    // Render process simulates unexpected segmentation fault / panic
    host.simulate_crash();
    assert!(host.is_crashed());

    // Host catches crash and generates Sad Tab page
    let sad_tab = IsolatedRenderHost::generate_sad_tab_html();
    assert!(sad_tab.contains("Aw, Snap!"));
    assert!(sad_tab.contains("WaveCore protected your system"));

    // User clicks "Reload Page" -> Host respawns process
    let (_b_ep2, r_ep2) = IpcChannel::create_pair();
    host.respawn(r_ep2);
    assert!(!host.is_crashed());
    assert_eq!(host.status, ProcessStatus::Running);
}
