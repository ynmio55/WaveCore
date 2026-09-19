use wavecore_dom::Node;

/// WaveCore's first deliberately small HTML parser.
/// v0.1 supports nested start/end tags and text. The tokenizer/tree builder
/// will be expanded toward the HTML Standard incrementally.
pub fn parse(input: &str) -> Node {
    let mut parser = Parser { input, pos: 0 };
    Node::document(parser.parse_nodes(None))
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn parse_nodes(&mut self, closing: Option<&str>) -> Vec<Node> {
        let mut nodes = Vec::new();

        while self.pos < self.input.len() {
            if self.starts_with("</") {
                self.pos += 2;
                let name = self.consume_until('>');
                self.consume_char('>');
                if closing.is_some_and(|tag| tag.eq_ignore_ascii_case(name.trim())) {
                    break;
                }
                continue;
            }

            if self.starts_with("<") {
                self.pos += 1;
                let name = self.consume_until('>');
                self.consume_char('>');
                let tag = name.trim().split_whitespace().next().unwrap_or("").to_ascii_lowercase();
                if !tag.is_empty() {
                    let children = self.parse_nodes(Some(&tag));
                    nodes.push(Node::element(tag, children));
                }
            } else {
                let text = self.consume_until('<');
                if !text.is_empty() {
                    nodes.push(Node::text(text));
                }
            }
        }

        nodes
    }

    fn starts_with(&self, s: &str) -> bool {
        self.input[self.pos..].starts_with(s)
    }

    fn consume_until(&mut self, ch: char) -> String {
        let start = self.pos;
        while self.pos < self.input.len() && self.input[self.pos..].chars().next() != Some(ch) {
            self.pos += self.input[self.pos..].chars().next().unwrap().len_utf8();
        }
        self.input[start..self.pos].to_owned()
    }

    fn consume_char(&mut self, expected: char) {
        if self.input[self.pos..].chars().next() == Some(expected) {
            self.pos += expected.len_utf8();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wavecore_dom::NodeType;

    #[test]
    fn parses_nested_elements() {
        let dom = parse("<main><h1>WaveCore</h1><p>Hello</p></main>");
        assert_eq!(dom.children.len(), 1);
        let NodeType::Element(main) = &dom.children[0].node_type else { panic!() };
        assert_eq!(main.tag_name, "main");
        assert_eq!(dom.children[0].children.len(), 2);
    }
}
