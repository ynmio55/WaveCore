use std::collections::BTreeMap;
use wavecore_css::{parse_declarations, Stylesheet};
use wavecore_dom::{ElementData, Node, NodeType};

#[derive(Debug, Clone)]
pub struct StyledNode {
    pub node: Node,
    pub properties: BTreeMap<String, String>,
    pub children: Vec<StyledNode>,
}

pub fn style_tree(node: &Node, sheet: &Stylesheet) -> StyledNode {
    style_tree_inner(node, sheet, &[], &BTreeMap::new())
}

fn style_tree_inner(
    node: &Node,
    sheet: &Stylesheet,
    ancestors: &[ElementData],
    inherited_custom: &BTreeMap<String, String>,
) -> StyledNode {
    let mut properties = BTreeMap::new();
    let mut custom = inherited_custom.clone();

    if let NodeType::Element(element) = &node.node_type {
        let mut matches: Vec<_> = sheet
            .rules
            .iter()
            .enumerate()
            .filter_map(|(order, rule)| {
                selector_specificity(&rule.selector, element, ancestors)
                    .map(|specificity| (specificity, order, rule))
            })
            .collect();
        matches.sort_by_key(|(specificity, order, _)| (*specificity, *order));

        for (_, _, rule) in matches {
            for (name, value) in &rule.declarations {
                if name.starts_with("--") {
                    custom.insert(name.clone(), value.clone());
                } else {
                    properties.insert(name.clone(), value.clone());
                }
            }
        }

        if let Some(inline_css) = element.attributes.get("style") {
            for (name, value) in parse_declarations(inline_css) {
                if name.starts_with("--") {
                    custom.insert(name, value);
                } else {
                    properties.insert(name, value);
                }
            }
        }

        for (name, value) in custom.clone() {
            properties.insert(name, value);
        }

        let snapshot = properties.clone();
        for (name, value) in snapshot {
            if !name.starts_with("--") {
                properties.insert(name, resolve_vars(&value, &custom, 0));
            }
        }
    } else {
        for (name, value) in &custom {
            properties.insert(name.clone(), value.clone());
        }
    }

    let mut next_ancestors = ancestors.to_vec();
    if let NodeType::Element(element) = &node.node_type {
        next_ancestors.push(element.clone());
    }

    let children = node
        .children
        .iter()
        .map(|child| style_tree_inner(child, sheet, &next_ancestors, &custom))
        .collect();

    StyledNode {
        node: node.clone(),
        properties,
        children,
    }
}

fn resolve_vars(value: &str, custom: &BTreeMap<String, String>, depth: usize) -> String {
    if depth > 16 {
        return value.to_string();
    }

    let mut out = value.to_string();
    loop {
        let Some(start) = out.find("var(") else {
            break;
        };
        let Some(end_rel) = out[start + 4..].find(')') else {
            break;
        };
        let end = start + 4 + end_rel;
        let inside = &out[start + 4..end];
        let (name, fallback) = inside
            .split_once(',')
            .map(|(a, b)| (a.trim(), Some(b.trim())))
            .unwrap_or((inside.trim(), None));

        let replacement = custom
            .get(name)
            .map(|v| resolve_vars(v, custom, depth + 1))
            .or_else(|| fallback.map(|f| resolve_vars(f, custom, depth + 1)))
            .unwrap_or_default();

        out.replace_range(start..=end, &replacement);
    }
    out
}

fn selector_specificity(
    selector: &str,
    element: &ElementData,
    ancestors: &[ElementData],
) -> Option<(u16, u16, u16)> {
    if selector.contains(',') {
        return selector
            .split(',')
            .filter_map(|part| match_complex_selector(part.trim(), element, ancestors))
            .max();
    }
    match_complex_selector(selector.trim(), element, ancestors)
}

fn match_complex_selector(
    selector: &str,
    element: &ElementData,
    ancestors: &[ElementData],
) -> Option<(u16, u16, u16)> {
    let normalized = selector.replace('>', " > ");
    let tokens: Vec<&str> = normalized.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }

    let mut compounds = Vec::new();
    let mut combinators = Vec::new();
    let mut expect_compound = true;
    for token in tokens {
        if token == ">" {
            combinators.push('>');
            expect_compound = true;
        } else {
            if !expect_compound && compounds.len() > combinators.len() {
                combinators.push(' ');
            }
            compounds.push(token);
            expect_compound = false;
        }
    }

    if compounds.is_empty() || !matches_compound(compounds.last().unwrap(), element) {
        return None;
    }

    let mut specificity = compound_specificity(compounds.last().unwrap());
    let mut anc_index = ancestors.len();
    for idx in (0..compounds.len().saturating_sub(1)).rev() {
        let combinator = combinators.get(idx).copied().unwrap_or(' ');
        let target = compounds[idx];

        match combinator {
            '>' => {
                if anc_index == 0 {
                    return None;
                }
                anc_index -= 1;
                if !matches_compound(target, &ancestors[anc_index]) {
                    return None;
                }
            }
            _ => {
                let mut found = None;
                while anc_index > 0 {
                    anc_index -= 1;
                    if matches_compound(target, &ancestors[anc_index]) {
                        found = Some(anc_index);
                        break;
                    }
                }
                if found.is_none() {
                    return None;
                }
            }
        }

        let part = compound_specificity(target);
        specificity.0 += part.0;
        specificity.1 += part.1;
        specificity.2 += part.2;
    }
    Some(specificity)
}

fn compound_specificity(selector: &str) -> (u16, u16, u16) {
    let mut ids = 0;
    let mut classes = 0;
    let mut tags = 0;
    let mut chars = selector.chars().peekable();
    let mut saw_tag = false;

    while let Some(ch) = chars.next() {
        match ch {
            '#' => ids += 1,
            '.' | '[' | ':' => classes += 1,
            '*' => {}
            c if (c.is_ascii_alphabetic() || c == '_') && !saw_tag => {
                tags += 1;
                saw_tag = true;
            }
            _ => {}
        }
    }
    (ids, classes, tags)
}

fn matches_compound(selector: &str, element: &ElementData) -> bool {
    if selector == "*" {
        return true;
    }
    if selector == ":root" {
        return element.tag_name.eq_ignore_ascii_case("html");
    }

    let mut rest = selector;
    if let Some(pos) = rest.find(['#', '.', '[', ':']) {
        let tag = &rest[..pos];
        if !tag.is_empty() && tag != "*" && !tag.eq_ignore_ascii_case(&element.tag_name) {
            return false;
        }
        rest = &rest[pos..];
    } else if !rest.is_empty() {
        return rest.eq_ignore_ascii_case(&element.tag_name);
    }

    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('#') {
            let end = after.find(['#', '.', '[', ':']).unwrap_or(after.len());
            if element.id() != Some(&after[..end]) {
                return false;
            }
            rest = &after[end..];
        } else if let Some(after) = rest.strip_prefix('.') {
            let end = after.find(['#', '.', '[', ':']).unwrap_or(after.len());
            if !element.has_class(&after[..end]) {
                return false;
            }
            rest = &after[end..];
        } else if let Some(after) = rest.strip_prefix('[') {
            let Some(end) = after.find(']') else {
                return false;
            };
            let expr = after[..end].trim();
            if let Some((name, expected)) = expr.split_once('=') {
                let expected = expected.trim().trim_matches(['"', '\'']);
                if element.get_attribute(name.trim()) != Some(expected) {
                    return false;
                }
            } else if element.get_attribute(expr).is_none() {
                return false;
            }
            rest = &after[end + 1..];
        } else if let Some(after) = rest.strip_prefix(":root") {
            if !element.tag_name.eq_ignore_ascii_case("html") {
                return false;
            }
            rest = after;
        } else {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use wavecore_css::parse;
    use wavecore_html::parse as html;

    #[test]
    fn id_beats_class_and_tag() {
        let d = html("<p id=\"x\" class=\"note\">x</p>");
        let s = style_tree(&d, &parse("p{color:black}.note{color:blue}#x{color:red}"));
        assert_eq!(s.children[0].properties.get("color").map(String::as_str), Some("red"));
    }

    #[test]
    fn inline_style_beats_id_rule() {
        let d = html("<p id=\"x\" style=\"color: gold; font-size: 20px;\">x</p>");
        let s = style_tree(&d, &parse("#x{color: red; margin: 10px;}"));
        assert_eq!(s.children[0].properties.get("color").map(String::as_str), Some("gold"));
        assert_eq!(s.children[0].properties.get("margin").map(String::as_str), Some("10px"));
    }

    #[test]
    fn descendant_and_child_selectors_use_real_ancestry() {
        let d = html("<main class=\"app\"><section><p class=\"note\">x</p></section></main>");
        let s = style_tree(
            &d,
            &parse(".app .note { color: green; } main > section { padding: 4px; }"),
        );
        assert_eq!(
            s.children[0].children[0].children[0]
                .properties
                .get("color")
                .map(String::as_str),
            Some("green")
        );
        assert_eq!(
            s.children[0].children[0]
                .properties
                .get("padding")
                .map(String::as_str),
            Some("4px")
        );
    }

    #[test]
    fn attribute_selectors_and_custom_properties_work() {
        let d = html(
            "<html><body><button data-kind=\"primary\">Go</button></body></html>",
        );
        let s = style_tree(
            &d,
            &parse(
                ":root { --accent: #3366ff; } button[data-kind=primary] { color: var(--accent); border-color: var(--missing, black); }",
            ),
        );
        let button = &s.children[0].children[0].children[0];
        assert_eq!(button.properties.get("color").map(String::as_str), Some("#3366ff"));
        assert_eq!(
            button.properties.get("border-color").map(String::as_str),
            Some("black")
        );
    }
}
