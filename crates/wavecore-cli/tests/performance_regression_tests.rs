//! Deterministic performance regressions for hot paths.
//!
//! These tests validate that the optimization mechanisms are actually used. They
//! intentionally avoid wall-clock timing so CI stays stable across machines.

use wavecore_css::parse as parse_css;
use wavecore_dom::Node;
use wavecore_layout::{LayoutCache, Rect};
use wavecore_pixels::Surface;
use wavecore_style::style_tree;

#[test]
fn performance_layout_cache_reuses_clean_tree() {
    let dom = Node::element(
        "div",
        (0..64)
            .map(|i| Node::element("p", vec![Node::text(format!("row {i}"))]))
            .collect(),
    );
    let css = parse_css("div { width: 800px; } p { margin: 2px; }");
    let styled = style_tree(&dom, &css);
    let mut cache = LayoutCache::new();

    let first = cache.layout(&styled, 1024.0, false);
    let second = cache.layout(&styled, 1024.0, false);

    assert_eq!(first.rect, second.rect);
    assert_eq!(cache.stats.misses, 1);
    assert_eq!(cache.stats.hits, 1);
    assert!(cache.hit_rate() >= 0.5);
}

#[test]
fn performance_damage_tracker_coalesces_adjacent_regions() {
    let mut surface = Surface::new(800, 600);
    for i in 0..100 {
        surface.mark_damage(Rect {
            x: i as f32 * 4.0,
            y: 20.0,
            width: 4.0,
            height: 30.0,
        });
    }

    let normalized = surface.normalized_damage();
    assert_eq!(normalized.len(), 1);
    assert!(normalized[0].width >= 400.0);
}
