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
}
