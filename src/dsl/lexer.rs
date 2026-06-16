use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Number(f64),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Gt,
    Lt,
    Ge,
    Le,
    EqEq,
    Ne,
    And,
    Or,
    Not,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
}

pub fn tokenize(src: &str) -> Result<Vec<Token>> {
    let chars: Vec<char> = src.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\n' | '\r' => i += 1,
            '+' => { tokens.push(Token::Plus); i += 1; }
            '-' => { tokens.push(Token::Minus); i += 1; }
            '*' => { tokens.push(Token::Star); i += 1; }
            '/' => { tokens.push(Token::Slash); i += 1; }
            '(' => { tokens.push(Token::LParen); i += 1; }
            ')' => { tokens.push(Token::RParen); i += 1; }
            '[' => { tokens.push(Token::LBracket); i += 1; }
            ']' => { tokens.push(Token::RBracket); i += 1; }
            ',' => { tokens.push(Token::Comma); i += 1; }
            '>' => {
                if chars.get(i + 1) == Some(&'=') { tokens.push(Token::Ge); i += 2; }
                else { tokens.push(Token::Gt); i += 1; }
            }
            '<' => {
                if chars.get(i + 1) == Some(&'=') { tokens.push(Token::Le); i += 2; }
                else { tokens.push(Token::Lt); i += 1; }
            }
            '=' => {
                if chars.get(i + 1) == Some(&'=') { tokens.push(Token::EqEq); i += 2; }
                else { return Err(Error::Dsl("'=' must be '=='".into())); }
            }
            '!' => {
                if chars.get(i + 1) == Some(&'=') { tokens.push(Token::Ne); i += 2; }
                else { return Err(Error::Dsl("'!' must be '!='".into())); }
            }
            c if c.is_ascii_digit() => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') { i += 1; }
                let s: String = chars[start..i].iter().collect();
                let n: f64 = s.parse().map_err(|_| Error::Dsl(format!("bad number: {s}")))?;
                tokens.push(Token::Number(n));
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len()
                    && (chars[i].is_ascii_alphanumeric() || chars[i] == '_' || chars[i] == '.')
                { i += 1; }
                let s: String = chars[start..i].iter().collect();
                match s.as_str() {
                    "and" => tokens.push(Token::And),
                    "or" => tokens.push(Token::Or),
                    "not" => tokens.push(Token::Not),
                    _ => tokens.push(Token::Ident(s)),
                }
            }
            other => return Err(Error::Dsl(format!("unexpected char: {other}"))),
        }
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_expression_with_dotted_ident_and_keywords() {
        let toks = tokenize("ema(close,20) >= ctx.close and not down").unwrap();
        assert!(toks.contains(&Token::And));
        assert!(toks.contains(&Token::Not));
        assert!(toks.contains(&Token::Ge));
        assert!(toks.iter().any(|t| matches!(t, Token::Ident(s) if s == "ctx.close")));
        assert!(toks.iter().any(|t| matches!(t, Token::Number(n) if *n == 20.0)));
    }

    #[test]
    fn tokenizes_comparison_and_brackets() {
        let toks = tokenize("close[-1] < 10.5").unwrap();
        assert_eq!(toks[0], Token::Ident("close".to_string()));
        assert_eq!(toks[1], Token::LBracket);
        assert_eq!(toks[2], Token::Minus);
        assert_eq!(toks[3], Token::Number(1.0));
        assert_eq!(toks[4], Token::RBracket);
        assert_eq!(toks[5], Token::Lt);
    }

    #[test]
    fn multi_dot_ident_is_single_token() {
        // aux.idx.v must be one Ident, not split at the second dot
        let toks = tokenize("aux.idx.v > 0").unwrap();
        assert_eq!(toks[0], Token::Ident("aux.idx.v".to_string()));
        assert_eq!(toks[1], Token::Gt);
        assert_eq!(toks[2], Token::Number(0.0));
    }

    #[test]
    fn fund_dotted_ident_is_single_token() {
        let toks = tokenize("fund.roe > 15").unwrap();
        assert_eq!(toks[0], Token::Ident("fund.roe".to_string()));
    }
}
