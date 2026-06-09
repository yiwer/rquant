use crate::dsl::ast::{BinaryOp, Expr, UnaryOp};
use crate::features::context::Context;
use crate::features::indicators;
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Series(Vec<f64>),
    Scalar(f64),
    Bool(bool),
}

/// Evaluate the expression and coerce the result to bool (for branch conditions).
pub fn eval_bool(expr: &Expr, ctx: &Context) -> Result<bool> {
    as_bool(&eval(expr, ctx)?)
}

pub fn eval(expr: &Expr, ctx: &Context) -> Result<Value> {
    match expr {
        Expr::Number(n) => Ok(Value::Scalar(*n)),
        Expr::Ident(name) => Ok(Value::Series(resolve_series(name, ctx)?)),
        Expr::Index(inner, k) => {
            let s = as_series(&eval(inner, ctx)?)?;
            let len = s.len() as i64;
            let pos = (len - 1) + *k;
            if pos < 0 || pos >= len {
                return Err(Error::Eval(format!("index {k} out of range (len {len})")));
            }
            Ok(Value::Scalar(s[pos as usize]))
        }
        Expr::Unary(op, e) => {
            let v = eval(e, ctx)?;
            match op {
                UnaryOp::Neg => Ok(Value::Scalar(-as_scalar(&v)?)),
                UnaryOp::Not => Ok(Value::Bool(!as_bool(&v)?)),
            }
        }
        Expr::Binary(op, l, r) => {
            let lv = eval(l, ctx)?;
            let rv = eval(r, ctx)?;
            Ok(match op {
                BinaryOp::And => Value::Bool(as_bool(&lv)? && as_bool(&rv)?),
                BinaryOp::Or => Value::Bool(as_bool(&lv)? || as_bool(&rv)?),
                BinaryOp::Add => Value::Scalar(as_scalar(&lv)? + as_scalar(&rv)?),
                BinaryOp::Sub => Value::Scalar(as_scalar(&lv)? - as_scalar(&rv)?),
                BinaryOp::Mul => Value::Scalar(as_scalar(&lv)? * as_scalar(&rv)?),
                BinaryOp::Div => Value::Scalar(as_scalar(&lv)? / as_scalar(&rv)?),
                BinaryOp::Gt => Value::Bool(as_scalar(&lv)? > as_scalar(&rv)?),
                BinaryOp::Lt => Value::Bool(as_scalar(&lv)? < as_scalar(&rv)?),
                BinaryOp::Ge => Value::Bool(as_scalar(&lv)? >= as_scalar(&rv)?),
                BinaryOp::Le => Value::Bool(as_scalar(&lv)? <= as_scalar(&rv)?),
                BinaryOp::Eq => Value::Bool(as_scalar(&lv)? == as_scalar(&rv)?),
                BinaryOp::Ne => Value::Bool(as_scalar(&lv)? != as_scalar(&rv)?),
            })
        }
        Expr::Call(name, args) => eval_call(name, args, ctx),
    }
}

fn resolve_series(name: &str, ctx: &Context) -> Result<Vec<f64>> {
    let (win, field) = match name.strip_prefix("ctx.") {
        Some(f) => (&ctx.context, f),
        None => (&ctx.primary, name),
    };
    match field {
        "close" => Ok(win.closes()),
        "open" => Ok(win.opens()),
        "high" => Ok(win.highs()),
        "low" => Ok(win.lows()),
        "volume" => Ok(win.volumes()),
        _ => Err(Error::Eval(format!("unknown identifier: {name}"))),
    }
}

/// Reduce a Value to a single f64.
/// Series: take the last element; empty/warm-up series → NaN so comparisons are
/// false and branches abstain (the intended "warm-up abstention" semantics).
fn as_scalar(v: &Value) -> Result<f64> {
    match v {
        Value::Scalar(x) => Ok(*x),
        Value::Series(s) => Ok(s.last().copied().unwrap_or(f64::NAN)),
        Value::Bool(_) => Err(Error::Eval("expected number, got bool".into())),
    }
}

fn as_bool(v: &Value) -> Result<bool> {
    match v {
        Value::Bool(b) => Ok(*b),
        _ => Err(Error::Eval("expected bool".into())),
    }
}

fn as_series(v: &Value) -> Result<Vec<f64>> {
    match v {
        Value::Series(s) => Ok(s.clone()),
        Value::Scalar(x) => Ok(vec![*x]),
        Value::Bool(_) => Err(Error::Eval("expected series".into())),
    }
}

fn as_usize(v: &Value) -> Result<usize> {
    let x = as_scalar(v)?;
    if x < 0.0 {
        return Err(Error::Eval("expected non-negative integer".into()));
    }
    Ok(x as usize)
}

fn need(args: &[Value], n: usize, name: &str) -> Result<()> {
    if args.len() != n {
        return Err(Error::Eval(format!("{name} expects {n} args, got {}", args.len())));
    }
    Ok(())
}

fn eval_call(name: &str, args: &[Expr], ctx: &Context) -> Result<Value> {
    let vals: Result<Vec<Value>> = args.iter().map(|a| eval(a, ctx)).collect();
    let vals = vals?;
    match name {
        "sma" => {
            need(&vals, 2, name)?;
            Ok(Value::Series(indicators::sma(&as_series(&vals[0])?, as_usize(&vals[1])?)))
        }
        "ema" => {
            need(&vals, 2, name)?;
            Ok(Value::Series(indicators::ema(&as_series(&vals[0])?, as_usize(&vals[1])?)))
        }
        // wma uses sma as a stand-in (documented design decision; do not fix)
        "wma" => {
            need(&vals, 2, name)?;
            Ok(Value::Series(indicators::sma(&as_series(&vals[0])?, as_usize(&vals[1])?)))
        }
        "rsi" => {
            need(&vals, 2, name)?;
            Ok(Value::Series(indicators::rsi(&as_series(&vals[0])?, as_usize(&vals[1])?)))
        }
        "atr" => {
            need(&vals, 1, name)?;
            let n = as_usize(&vals[0])?;
            Ok(Value::Series(indicators::atr(
                &ctx.primary.highs(),
                &ctx.primary.lows(),
                &ctx.primary.closes(),
                n,
            )))
        }
        "slope" => {
            need(&vals, 2, name)?;
            Ok(Value::Scalar(indicators::slope(&as_series(&vals[0])?, as_usize(&vals[1])?)))
        }
        "highest" => {
            need(&vals, 2, name)?;
            Ok(Value::Scalar(indicators::highest(&as_series(&vals[0])?, as_usize(&vals[1])?)))
        }
        "lowest" => {
            need(&vals, 2, name)?;
            Ok(Value::Scalar(indicators::lowest(&as_series(&vals[0])?, as_usize(&vals[1])?)))
        }
        "crossover" => {
            need(&vals, 2, name)?;
            Ok(Value::Bool(indicators::crossover(&as_series(&vals[0])?, &as_series(&vals[1])?)))
        }
        "crossunder" => {
            need(&vals, 2, name)?;
            Ok(Value::Bool(indicators::crossunder(&as_series(&vals[0])?, &as_series(&vals[1])?)))
        }
        _ => Err(Error::Eval(format!("unknown function: {name}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::bar::{Bar, Window};
    use crate::dsl::parser::parse_str;
    use crate::features::context::Context;
    use chrono::NaiveDate;

    fn ctx_from_closes(closes: &[f64]) -> Context {
        let base = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        let bars: Vec<Bar> = closes
            .iter()
            .enumerate()
            .map(|(i, &c)| Bar {
                time: base + chrono::Duration::minutes(i as i64 * 15),
                open: c, high: c, low: c, close: c, volume: 1.0,
            })
            .collect();
        let t = bars.last().unwrap().time;
        Context { t, primary: Window { bars: bars.clone() }, context: Window { bars }, news: None }
    }

    #[test]
    fn comparison_reduces_series_to_latest() {
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        let e = parse_str("close > sma(close,3)").unwrap();
        assert_eq!(eval(&e, &ctx).unwrap(), Value::Bool(true));
    }

    #[test]
    fn index_returns_previous_scalar() {
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        let e = parse_str("close[-1]").unwrap();
        assert_eq!(eval(&e, &ctx).unwrap(), Value::Scalar(4.0));
    }

    #[test]
    fn slope_of_series_is_scalar() {
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        let e = parse_str("slope(close,5)").unwrap();
        assert_eq!(eval(&e, &ctx).unwrap(), Value::Scalar(1.0));
    }

    #[test]
    fn and_of_bools_and_ctx_ident() {
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        let e = parse_str("close > 4 and ctx.close > 0").unwrap();
        assert_eq!(eval(&e, &ctx).unwrap(), Value::Bool(true));
    }
}
