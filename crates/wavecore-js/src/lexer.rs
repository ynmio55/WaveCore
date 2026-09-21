#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Literals
    Number(f64),
    String(String),
    Identifier(String),

    // Keywords
    Let,
    Const,
    Var,
    Function,
    Async,
    Await,
    Class,
    Return,
    If,
    Else,
    While,
    For,
    True,
    False,
    Null,
    Undefined,
    Try,
    Catch,
    Finally,
    Throw,
    New,
    TypeOf,
    InstanceOf,
    This,
    In,

    // Operators
    Plus,
    PlusEqual,
    Minus,
    MinusEqual,
    Star,
    StarEqual,
    Slash,
    SlashEqual,
    Percent,
    Equal,
    EqualEqual,
    EqualEqualEqual,
    NotEqual,
    NotEqualEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    AndAnd,
    OrOr,
    Bang,
    Dot,

    // Delimiters
    Comma,
    Semicolon,
    Colon,
    Question,
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,

    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
}

pub struct Lexer<'a> {
    _source: &'a str,
    chars: Vec<char>,
    cursor: usize,
    line: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            _source: source,
            chars: source.chars().collect(),
            cursor: 0,
            line: 1,
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, String> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace_and_comments();
            if self.is_at_end() {
                tokens.push(Token {
                    kind: TokenKind::Eof,
                    line: self.line,
                });
                break;
            }

            let start_line = self.line;
            let ch = self.advance();

            let kind = match ch {
                '(' => TokenKind::LeftParen,
                ')' => TokenKind::RightParen,
                '{' => TokenKind::LeftBrace,
                '}' => TokenKind::RightBrace,
                '[' => TokenKind::LeftBracket,
                ']' => TokenKind::RightBracket,
                ',' => TokenKind::Comma,
                ';' => TokenKind::Semicolon,
                ':' => TokenKind::Colon,
                '?' => TokenKind::Question,
                '.' => TokenKind::Dot,
                '+' => {
                    if self.match_char('=') {
                        TokenKind::PlusEqual
                    } else {
                        TokenKind::Plus
                    }
                }
                '-' => {
                    if self.match_char('=') {
                        TokenKind::MinusEqual
                    } else {
                        TokenKind::Minus
                    }
                }
                '*' => {
                    if self.match_char('=') {
                        TokenKind::StarEqual
                    } else {
                        TokenKind::Star
                    }
                }
                '/' => {
                    if self.match_char('=') {
                        TokenKind::SlashEqual
                    } else {
                        TokenKind::Slash
                    }
                }
                '%' => TokenKind::Percent,
                '!' => {
                    if self.match_char('=') {
                        if self.match_char('=') {
                            TokenKind::NotEqualEqual
                        } else {
                            TokenKind::NotEqual
                        }
                    } else {
                        TokenKind::Bang
                    }
                }
                '=' => {
                    if self.match_char('=') {
                        if self.match_char('=') {
                            TokenKind::EqualEqualEqual
                        } else {
                            TokenKind::EqualEqual
                        }
                    } else {
                        TokenKind::Equal
                    }
                }
                '<' => {
                    if self.match_char('=') {
                        TokenKind::LessEqual
                    } else {
                        TokenKind::Less
                    }
                }
                '>' => {
                    if self.match_char('=') {
                        TokenKind::GreaterEqual
                    } else {
                        TokenKind::Greater
                    }
                }
                '&' => {
                    if self.match_char('&') {
                        TokenKind::AndAnd
                    } else {
                        return Err(format!("Unexpected single '&' at line {}", self.line));
                    }
                }
                '|' => {
                    if self.match_char('|') {
                        TokenKind::OrOr
                    } else {
                        return Err(format!("Unexpected single '|' at line {}", self.line));
                    }
                }
                '"' | '\'' | '`' => self.string(ch)?,
                c if c.is_ascii_digit() => self.number(c)?,
                c if c.is_alphabetic() || c == '_' || c == '$' => self.identifier(c),
                _ => return Err(format!("Unexpected character '{}' at line {}", ch, self.line)),
            };

            tokens.push(Token {
                kind,
                line: start_line,
            });
        }

        Ok(tokens)
    }

    fn skip_whitespace_and_comments(&mut self) {
        while !self.is_at_end() {
            match self.peek() {
                ' ' | '\t' | '\r' => {
                    self.advance();
                }
                '\n' => {
                    self.line += 1;
                    self.advance();
                }
                '/' => {
                    if self.peek_next() == Some('/') {
                        // Line comment
                        while !self.is_at_end() && self.peek() != '\n' {
                            self.advance();
                        }
                    } else if self.peek_next() == Some('*') {
                        // Block comment
                        self.advance(); // consume '/'
                        self.advance(); // consume '*'
                        while !self.is_at_end() {
                            if self.peek() == '*' && self.peek_next() == Some('/') {
                                self.advance();
                                self.advance();
                                break;
                            }
                            if self.peek() == '\n' {
                                self.line += 1;
                            }
                            self.advance();
                        }
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }
    }

    fn string(&mut self, quote: char) -> Result<TokenKind, String> {
        let mut s = String::new();
        while !self.is_at_end() && self.peek() != quote {
            let c = self.advance();
            if c == '\\' && !self.is_at_end() {
                let esc = self.advance();
                match esc {
                    'n' => s.push('\n'),
                    't' => s.push('\t'),
                    'r' => s.push('\r'),
                    '\\' => s.push('\\'),
                    '\'' => s.push('\''),
                    '"' => s.push('"'),
                    _ => s.push(esc),
                }
            } else {
                if c == '\n' {
                    self.line += 1;
                }
                s.push(c);
            }
        }

        if self.is_at_end() {
            return Err(format!("Unterminated string at line {}", self.line));
        }

        self.advance(); // consume closing quote
        Ok(TokenKind::String(s))
    }

    fn number(&mut self, first: char) -> Result<TokenKind, String> {
        let mut s = String::new();
        s.push(first);
        while !self.is_at_end() && self.peek().is_ascii_digit() {
            s.push(self.advance());
        }
        if self.peek() == '.' && self.peek_next().map_or(false, |c| c.is_ascii_digit()) {
            s.push(self.advance()); // consume '.'
            while !self.is_at_end() && self.peek().is_ascii_digit() {
                s.push(self.advance());
            }
        }
        let num: f64 = s.parse().map_err(|e| format!("Invalid number '{}': {}", s, e))?;
        Ok(TokenKind::Number(num))
    }

    fn identifier(&mut self, first: char) -> TokenKind {
        let mut s = String::new();
        s.push(first);
        while !self.is_at_end() {
            let c = self.peek();
            if c.is_alphanumeric() || c == '_' || c == '$' {
                s.push(self.advance());
            } else {
                break;
            }
        }

        match s.as_str() {
            "let" => TokenKind::Let,
            "const" => TokenKind::Const,
            "var" => TokenKind::Var,
            "function" => TokenKind::Function,
            "async" => TokenKind::Async,
            "await" => TokenKind::Await,
            "class" => TokenKind::Class,
            "return" => TokenKind::Return,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "while" => TokenKind::While,
            "for" => TokenKind::For,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "null" => TokenKind::Null,
            "undefined" => TokenKind::Undefined,
            "try" => TokenKind::Try,
            "catch" => TokenKind::Catch,
            "finally" => TokenKind::Finally,
            "throw" => TokenKind::Throw,
            "new" => TokenKind::New,
            "typeof" => TokenKind::TypeOf,
            "instanceof" => TokenKind::InstanceOf,
            "this" => TokenKind::This,
            "in" => TokenKind::In,
            _ => TokenKind::Identifier(s),
        }
    }

    fn advance(&mut self) -> char {
        let ch = self.chars[self.cursor];
        self.cursor += 1;
        ch
    }

    fn match_char(&mut self, expected: char) -> bool {
        if self.is_at_end() || self.chars[self.cursor] != expected {
            false
        } else {
            self.cursor += 1;
            true
        }
    }

    fn peek(&self) -> char {
        if self.is_at_end() {
            '\0'
        } else {
            self.chars[self.cursor]
        }
    }

    fn peek_next(&self) -> Option<char> {
        if self.cursor + 1 < self.chars.len() {
            Some(self.chars[self.cursor + 1])
        } else {
            None
        }
    }

    fn is_at_end(&self) -> bool {
        self.cursor >= self.chars.len()
    }
}
