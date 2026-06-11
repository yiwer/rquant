use std::collections::HashMap;

/// 一元运算符。
#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOp {
    /// 逻辑非 `!`。
    Not,
    /// 算术取反 `-`。
    Neg,
}

/// 二元运算符。
#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOp {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `>`
    Gt,
    /// `<`
    Lt,
    /// `>=`
    Ge,
    /// `<=`
    Le,
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// `&&`
    And,
    /// `||`
    Or,
}

/// DSL 表达式 AST 节点。
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// 数值字面量。
    Number(f64),
    /// 标识符，对应 context 中的命名序列（如 `close`、`volume`）。
    Ident(String),
    /// 序列索引：`expr[i]`。负数索引从尾部倒数（`-1` = 最新 bar，`-2` = 次新，以此类推）。
    Index(Box<Expr>, i64),
    /// 一元表达式。
    Unary(UnaryOp, Box<Expr>),
    /// 二元表达式。
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    /// 函数调用（如 `sma(close, 20)`、`sigmoid(x)`）。
    Call(String, Vec<Expr>),
    /// 因子缓存槽：loader 给每个因子体包一层；同一 Context 内首算后命中（dsl/eval.rs）。
    Cached(u32, Box<Expr>),
}

/// 把表达式中的 Ident(name) 按 env 替换为对应子树（深拷贝）；params/factors 加载期内联用。
pub fn substitute(expr: &Expr, env: &HashMap<String, Expr>) -> Expr {
    match expr {
        Expr::Ident(name) => env.get(name).cloned().unwrap_or_else(|| expr.clone()),
        Expr::Number(_) => expr.clone(),
        Expr::Unary(op, e) => Expr::Unary(op.clone(), Box::new(substitute(e, env))),
        Expr::Binary(op, l, r) => {
            Expr::Binary(op.clone(), Box::new(substitute(l, env)), Box::new(substitute(r, env)))
        }
        Expr::Call(name, args) => {
            Expr::Call(name.clone(), args.iter().map(|a| substitute(a, env)).collect())
        }
        Expr::Index(e, k) => Expr::Index(Box::new(substitute(e, env)), *k),
        Expr::Cached(id, e) => Expr::Cached(*id, Box::new(substitute(e, env))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitute_params_and_nested() {
        use crate::dsl::parser::parse_str;
        let mut env = HashMap::new();
        env.insert("n".to_string(), Expr::Number(20.0));
        let e = substitute(&parse_str("sma(close, n) > n").unwrap(), &env);
        // n 全部替换为 20，close 保留
        let rendered = format!("{e:?}");
        assert!(!rendered.contains("Ident(\"n\")"));
        assert!(rendered.contains("Ident(\"close\")"));
        assert!(rendered.contains("Number(20.0)"));
    }
}
