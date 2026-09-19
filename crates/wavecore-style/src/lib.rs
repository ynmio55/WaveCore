use std::collections::BTreeMap;
use wavecore_css::Stylesheet;
use wavecore_dom::{ElementData, Node, NodeType};

#[derive(Debug, Clone)]
pub struct StyledNode {
    pub node: Node,
    pub properties: BTreeMap<String, String>,
    pub children: Vec<StyledNode>,
}

pub fn style_tree(node: &Node, sheet: &Stylesheet) -> StyledNode {
    let mut properties = BTreeMap::new();
    if let NodeType::Element(element) = &node.node_type {
        let mut matches: Vec<_> = sheet
            .rules
            .iter()
            .enumerate()
            .filter_map(|(order, r)| selector_specificity(&r.selector, element).map(|s| (s, order, r)))
            .collect();
        matches.sort_by_key(|(s, o, _)| (*s, *o));
        for (_, _, rule) in matches {
            properties.extend(rule.declarations.clone());
        }

        // Inline style="..." attribute has highest cascade priority
        if let Some(inline_css) = element.attributes.get("style") {
            for decl in inline_css.split(';') {
                if let Some((name, val)) = decl.split_once(':') {
                    let name = name.trim().to_ascii_lowercase();
                    let val = val.trim();
                    if !name.is_empty() && !val.is_empty() {
                        properties.insert(name, val.to_string());
                    }
                }
            }
        }
    }

    StyledNode {
        node: node.clone(),
        properties,
        children: node.children.iter().map(|c| style_tree(c, sheet)).collect(),
    }
}

fn selector_specificity(selector: &str, e: &ElementData) -> Option<(u16, u16, u16)> {
    if selector.contains(',') {
        let mut best: Option<(u16, u16, u16)> = None;
        for part in selector.split(',') {
            if let Some(spec) = match_single_selector(part.trim(), e) {
                best = Some(best.map_or(spec, |b| b.max(spec)));
            }
        }
        return best;
    }
    match_single_selector(selector.trim(), e)
}

fn match_single_selector(s: &str, e: &ElementData) -> Option<(u16, u16, u16)> {
    let s = s.trim();
    if s.contains(' ') {
        if let Some((_, last)) = s.rsplit_once(' ') {
            return match_single_selector(last.trim(), e);
        }
    }
    if s == "*" {
        return Some((0, 0, 0));
    }
    if let Some(id) = s.strip_prefix('#') {
        return (e.id() == Some(id)).then_some((1, 0, 0));
    }
    if let Some(class) = s.strip_prefix('.') {
        if class.contains('.') {
            let classes: Vec<&str> = class.split('.').collect();
            let all_match = classes.iter().all(|c| e.has_class(c));
            return all_match.then_some((0, classes.len() as u16, 0));
        }
        return e.has_class(class).then_some((0, 1, 0));
    }
    if let Some((tag, class)) = s.split_once('.') {
        if tag.eq_ignore_ascii_case(&e.tag_name) && e.has_class(class) {
            return Some((0, 1, 1));
        }
        return None;
    }
    s.eq_ignore_ascii_case(&e.tag_name).then_some((0, 0, 1))
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
    fn comma_and_tag_class_selectors() {
        let d = html("<h1 class=\"title\">Header</h1>");
        let s = style_tree(&d, &parse("h1.title, h2.title { color: purple; }"));
        assert_eq!(s.children[0].properties.get("color").map(String::as_str), Some("purple"));
    }
}
