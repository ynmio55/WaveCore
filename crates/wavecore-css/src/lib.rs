use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Stylesheet { pub rules: Vec<Rule> }

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub selector: String,
    pub declarations: BTreeMap<String, String>,
}

pub fn parse(input: &str) -> Stylesheet {
    let mut rules = Vec::new();
    for chunk in input.split('}') {
        let Some((selector, body)) = chunk.split_once('{') else { continue };
        let selector = selector.trim();
        if selector.is_empty() { continue; }
        let declarations = body.split(';').filter_map(|d| {
            let (name, value) = d.split_once(':')?;
            let name = name.trim();
            let value = value.trim();
            (!name.is_empty() && !value.is_empty()).then(|| (name.to_ascii_lowercase(), value.to_string()))
        }).collect();
        rules.push(Rule { selector: selector.to_string(), declarations });
    }
    Stylesheet { rules }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_rule() {
        let css = parse("h1 { color: red; margin: 8px; }");
        assert_eq!(css.rules[0].declarations["color"], "red");
    }
}
