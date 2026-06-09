use crate::dsl::ast::{BinaryOp, Expr, UnaryOp};
use crate::dsl::lexer::{tokenize, Token};
use crate::{Error, Result};

/// Convenience entry point: source string -> AST.
pub fn parse_str(src: &str) -> Result<Expr> {
    let tokens = tokenize(src)?;
    Parser::new(tokens).parse()
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn expect(&mut self, t: Token) -> Result<()> {
        if self.peek() == Some(&t) {
            self.pos += 1;
            Ok(())
        } else {
            Err(Error::Dsl(format!("expected {t:?}, got {:?}", self.peek())))
        }
    }

    pub fn parse(&mut self) -> Result<Expr> {
        let e = self.parse_expr(0)?;
        if self.pos != self.tokens.len() {
            return Err(Error::Dsl("trailing tokens after expression".into()));
        }
        Ok(e)
    }

    fn parse_expr(&mut self, min_bp: u8) -> Result<Expr> {
        let mut lhs = self.parse_prefix()?;
        loop {
            let (lbp, rbp, op) = match self.peek() {
                Some(Token::Or) => (1, 2, BinaryOp::Or),
                Some(Token::And) => (3, 4, BinaryOp::And),
                Some(Token::Gt) => (5, 6, BinaryOp::Gt),
                Some(Token::Lt) => (5, 6, BinaryOp::Lt),
                Some(Token::Ge) => (5, 6, BinaryOp::Ge),
                Some(Token::Le) => (5, 6, BinaryOp::Le),
                Some(Token::EqEq) => (5, 6, BinaryOp::Eq),
                Some(Token::Ne) => (5, 6, BinaryOp::Ne),
                Some(Token::Plus) => (7, 8, BinaryOp::Add),
                Some(Token::Minus) => (7, 8, BinaryOp::Sub),
                Some(Token::Star) => (9, 10, BinaryOp::Mul),
                Some(Token::Slash) => (9, 10, BinaryOp::Div),
                _ => break,
            };
            if lbp < min_bp {
                break;
            }
            self.pos += 1;
            let rhs = self.parse_expr(rbp)?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_prefix(&mut self) -> Result<Expr> {
        match self.next() {
            Some(Token::Not) => {
                let e = self.parse_expr(11)?;
                Ok(Expr::Unary(UnaryOp::Not, Box::new(e)))
            }
            Some(Token::Minus) => {
                let e = self.parse_expr(11)?;
                Ok(Expr::Unary(UnaryOp::Neg, Box::new(e)))
            }
            Some(Token::Number(n)) => self.parse_postfix(Expr::Number(n)),
            Some(Token::LParen) => {
                let e = self.parse_expr(0)?;
                self.expect(Token::RParen)?;
                self.parse_postfix(e)
            }
            Some(Token::Ident(name)) => {
                if self.peek() == Some(&Token::LParen) {
                    self.pos += 1;
                    let mut args = Vec::new();
                    if self.peek() != Some(&Token::RParen) {
                        loop {
                            args.push(self.parse_expr(0)?);
                            if self.peek() == Some(&Token::Comma) {
                                self.pos += 1;
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect(Token::RParen)?;
                    self.parse_postfix(Expr::Call(name, args))
                } else {
                    self.parse_postfix(Expr::Ident(name))
                }
            }
            other => Err(Error::Dsl(format!("unexpected token: {other:?}"))),
        }
    }

    fn parse_postfix(&mut self, e: Expr) -> Result<Expr> {
        let mut e = e;
        while self.peek() == Some(&Token::LBracket) {
            self.pos += 1;
            let neg = if self.peek() == Some(&Token::Minus) {
                self.pos += 1;
                true
            } else {
                false
            };
            let idx = match self.next() {
                Some(Token::Number(n)) => n as i64,
                other => return Err(Error::Dsl(format!("expected index number, got {other:?}"))),
            };
            let idx = if neg { -idx } else { idx };
            self.expect(Token::RBracket)?;
            e = Expr::Index(Box::new(e), idx);
        }
        Ok(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::ast::{BinaryOp, Expr};

    #[test]
    fn precedence_mul_binds_tighter_than_add() {
        let e = parse_str("1 + 2 * 3").unwrap();
        match e {
            Expr::Binary(BinaryOp::Add, l, r) => {
                assert_eq!(*l, Expr::Number(1.0));
                assert!(matches!(*r, Expr::Binary(BinaryOp::Mul, _, _)));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_call_and_comparison() {
        let e = parse_str("close > sma(close,5)").unwrap();
        match e {
            Expr::Binary(BinaryOp::Gt, l, r) => {
                assert_eq!(*l, Expr::Ident("close".into()));
                assert!(matches!(*r, Expr::Call(ref name, ref args) if name == "sma" && args.len() == 2));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_negative_index() {
        let e = parse_str("close[-1]").unwrap();
        assert_eq!(e, Expr::Index(Box::new(Expr::Ident("close".into())), -1));
    }
}
