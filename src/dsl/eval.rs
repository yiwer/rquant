use crate::dsl::ast::{BinaryOp, Expr, UnaryOp};
use crate::features::context::Context;
use crate::features::indicators;
use crate::{Error, Result};
use chrono::{Datelike, Timelike};

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

/// Evaluate and reduce to a single f64 (series → last). For strength expressions.
pub fn eval_scalar(expr: &Expr, ctx: &Context) -> Result<f64> {
    as_scalar(&eval(expr, ctx)?)
}

/// Fuzzy evaluation of boolean expressions → \[0,1\] truth value (soft quantization strength: "auto" use).
/// Comparisons: sigmoid((lhs-rhs)/denom), denom = scale·max(|lhs|,|rhs|); denom≈0 → 0.5.
/// and=min, or=max, not=1-x (Gödel); ==/!= stay hard; non-boolean nodes → Err.
pub fn eval_fuzzy(expr: &Expr, ctx: &Context, scale: f64) -> Result<f64> {
    match expr {
        Expr::Binary(op, l, r) => match op {
            BinaryOp::And => Ok(eval_fuzzy(l, ctx, scale)?.min(eval_fuzzy(r, ctx, scale)?)),
            BinaryOp::Or => Ok(eval_fuzzy(l, ctx, scale)?.max(eval_fuzzy(r, ctx, scale)?)),
            BinaryOp::Gt | BinaryOp::Ge => fuzzy_cmp(l, r, ctx, scale, 1.0),
            BinaryOp::Lt | BinaryOp::Le => fuzzy_cmp(l, r, ctx, scale, -1.0),
            BinaryOp::Eq | BinaryOp::Ne => Ok(if as_bool(&eval(expr, ctx)?)? { 1.0 } else { 0.0 }),
            _ => Err(Error::Eval("fuzzy: expected boolean expression".into())),
        },
        Expr::Unary(UnaryOp::Not, e) => Ok(1.0 - eval_fuzzy(e, ctx, scale)?),
        _ => Err(Error::Eval("fuzzy: expected boolean expression".into())),
    }
}

fn fuzzy_cmp(l: &Expr, r: &Expr, ctx: &Context, scale: f64, sign: f64) -> Result<f64> {
    let lv = as_scalar(&eval(l, ctx)?)?;
    let rv = as_scalar(&eval(r, ctx)?)?;
    let margin = (lv - rv) * sign;
    let denom = scale * lv.abs().max(rv.abs());
    if denom <= 1e-12 {
        return Ok(0.5);
    }
    Ok(1.0 / (1.0 + (-margin / denom).exp()))
}

pub fn eval(expr: &Expr, ctx: &Context) -> Result<Value> {
    match expr {
        Expr::Number(n) => Ok(Value::Scalar(*n)),
        Expr::Ident(name) => match name.as_str() {
            "hour" => Ok(Value::Scalar(f64::from(ctx.t.hour()))),
            "minute" => Ok(Value::Scalar(f64::from(ctx.t.minute()))),
            "dow" => Ok(Value::Scalar(f64::from(ctx.t.weekday().number_from_monday()))),
            "pos" => Ok(Value::Scalar(ctx.sim.pos)),
            "entry_price" => Ok(Value::Scalar(ctx.sim.entry_price)),
            "bars_held" => Ok(Value::Scalar(ctx.sim.bars_held as f64)),
            "unreal_pnl" => Ok(Value::Scalar(ctx.sim.unreal_pnl)),
            _ => Ok(Value::Series(resolve_series(name, ctx)?)),
        },
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
                BinaryOp::Eq => {
                    let (a, b) = (as_scalar(&lv)?, as_scalar(&rv)?);
                    Value::Bool(!a.is_nan() && !b.is_nan() && a == b)
                }
                BinaryOp::Ne => {
                    let (a, b) = (as_scalar(&lv)?, as_scalar(&rv)?);
                    Value::Bool(!a.is_nan() && !b.is_nan() && a != b)
                }

            })
        }
        Expr::Call(name, args) => eval_call(name, args, ctx),
    }
}

fn resolve_series(name: &str, ctx: &Context) -> Result<Vec<f64>> {
    if let Some(rest) = name.strip_prefix("aux.") {
        let (table, column) = rest
            .split_once('.')
            .ok_or_else(|| Error::Eval(format!("aux identifier must be aux.<table>.<column>: '{name}'")))?;
        let view = ctx.aux.get(table).ok_or_else(|| {
            Error::Eval(format!("aux table '{table}' not mounted (use --aux {table}=path.csv)"))
        })?;
        return view
            .cols
            .get(column)
            .cloned()
            .ok_or_else(|| Error::Eval(format!("aux table '{table}' has no column '{column}'")));
    }
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
/// Series: take the last element; empty/warm-up series → NaN. All comparisons (including explicit ==/!=)
/// return false on NaN → branches abstain during warm-up ("warm-up abstention" semantics).
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
        "wma" => {
            need(&vals, 2, name)?;
            Ok(Value::Series(indicators::wma(&as_series(&vals[0])?, as_usize(&vals[1])?)))
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
        "ref" => {
            need(&vals, 2, name)?;
            let s = as_series(&vals[0])?;
            let k = as_usize(&vals[1])?;
            let end = s.len().saturating_sub(k);
            Ok(Value::Series(s[..end].to_vec()))
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
        "macd_line" => { need(&vals, 3, name)?; Ok(Value::Series(indicators::macd_line(&as_series(&vals[0])?, as_usize(&vals[1])?, as_usize(&vals[2])?))) }
        "macd_signal" => { need(&vals, 4, name)?; Ok(Value::Series(indicators::macd_signal(&as_series(&vals[0])?, as_usize(&vals[1])?, as_usize(&vals[2])?, as_usize(&vals[3])?))) }
        "macd_hist" => { need(&vals, 4, name)?; Ok(Value::Series(indicators::macd_hist(&as_series(&vals[0])?, as_usize(&vals[1])?, as_usize(&vals[2])?, as_usize(&vals[3])?))) }
        "std" => { need(&vals, 2, name)?; Ok(Value::Scalar(indicators::std(&as_series(&vals[0])?, as_usize(&vals[1])?))) }
        "sigmoid" => {
            need(&vals, 1, name)?;
            Ok(Value::Scalar(1.0 / (1.0 + (-as_scalar(&vals[0])?).exp())))
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
        Context { t, primary: Window { bars: bars.clone() }, context: Window { bars }, news: None, aux: std::collections::BTreeMap::new(), sim: crate::features::context::SimState::default() }
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

    #[test]
    fn wma_std_macd_eval() {
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(eval(&parse_str("wma(close,3) > 0").unwrap(), &ctx).unwrap(), Value::Bool(true));
        match eval(&parse_str("std(close,5)").unwrap(), &ctx).unwrap() {
            Value::Scalar(x) => assert!((x - 2.0_f64.sqrt()).abs() < 1e-9),
            other => panic!("expected scalar, got {other:?}"),
        }
        assert_eq!(eval(&parse_str("macd_line(close,3,5) > -1000.0").unwrap(), &ctx).unwrap(), Value::Bool(true));
        assert_eq!(eval(&parse_str("macd_hist(close,3,5,2) > -1000.0").unwrap(), &ctx).unwrap(), Value::Bool(true));
    }

    #[test]
    fn sigmoid_eval() {
        let ctx = ctx_from_closes(&[1.0]);
        match eval(&parse_str("sigmoid(0)").unwrap(), &ctx).unwrap() {
            Value::Scalar(x) => assert!((x - 0.5).abs() < 1e-9),
            o => panic!("{o:?}"),
        }
        match eval(&parse_str("sigmoid(100)").unwrap(), &ctx).unwrap() {
            Value::Scalar(x) => assert!(x > 0.99),
            o => panic!("{o:?}"),
        }
    }

    #[test]
    fn fuzzy_comparison_and_combinators() {
        let ctx = ctx_from_closes(&[1.0]);
        let f = |src: &str| eval_fuzzy(&parse_str(src).unwrap(), &ctx, 0.02).unwrap();
        // 相等 → 0.5
        assert!((f("10 > 10") - 0.5).abs() < 1e-9);
        // above → >0.5 且单调；below → <0.5
        assert!(f("10.2 > 10") > 0.5);
        assert!(f("12 > 10") > f("10.2 > 10"));
        assert!(f("9.8 > 10") < 0.5);
        // 镜像
        assert!((f("10 < 10") - 0.5).abs() < 1e-9);
        assert!(f("9.8 < 10") > 0.5);
        // and=min / or=max / not=1-x
        let a = f("12 > 10");
        assert!((f("12 > 10 and 10 > 10") - 0.5).abs() < 1e-9);
        assert!((f("9.8 > 10 or 12 > 10") - a).abs() < 1e-9);
        assert!((f("not (10 > 10)") - 0.5).abs() < 1e-9);
        // == 保持硬
        assert!((f("10 == 10") - 1.0).abs() < 1e-9);
        assert!((f("10 == 11") - 0.0).abs() < 1e-9);
        // 双方≈0 → 0.5（无信息）
        assert!((f("0 > 0") - 0.5).abs() < 1e-9);
        // 非布尔 → Err
        assert!(eval_fuzzy(&parse_str("close").unwrap(), &ctx, 0.02).is_err());
    }

    #[test]
    fn nan_comparisons_abstain_including_ne() {
        // 3 bars, sma(close,10) is NaN (warm-up) → ALL comparisons must be false (abstention)
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0]);
        let f = |src: &str| eval(&parse_str(src).unwrap(), &ctx).unwrap();
        assert_eq!(f("sma(close,10) != 0"), Value::Bool(false)); // the bug: was true
        assert_eq!(f("sma(close,10) == sma(close,10)"), Value::Bool(false));
        assert_eq!(f("sma(close,10) > 0"), Value::Bool(false));
        // normal values unaffected
        assert_eq!(f("5 != 4"), Value::Bool(true));
        assert_eq!(f("5 == 5"), Value::Bool(true));
        assert_eq!(f("5 != 5"), Value::Bool(false));
    }

    #[test]
    fn ref_shifts_series_for_turtle_breakout() {
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        let f = |src: &str| eval(&parse_str(src).unwrap(), &ctx).unwrap();
        // ref(s, k):去掉末 k 根 → 末元素即 k 根前的值
        assert_eq!(f("ref(close, 1) == 4"), Value::Bool(true));
        assert_eq!(f("ref(close, 0) == 5"), Value::Bool(true));
        // Turtle 原义:close 高于"前 3 根"最高 → 可触发
        assert_eq!(f("close > highest(ref(close, 1), 3)"), Value::Bool(true));
        // 对照:含当前 bar 的写法恒假
        assert_eq!(f("close > highest(close, 3)"), Value::Bool(false));
    }

    #[test]
    fn ref_beyond_history_abstains() {
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0]);
        let f = |src: &str| eval(&parse_str(src).unwrap(), &ctx).unwrap();
        // k ≥ 序列长度 → 空序列 → NaN → 所有比较弃权
        assert_eq!(f("ref(close, 99) > 0"), Value::Bool(false));
        assert_eq!(f("ref(close, 99) == ref(close, 99)"), Value::Bool(false));
    }

    #[test]
    fn ref_wrong_arity_errors() {
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0]);
        let err = eval(&parse_str("ref(close)").unwrap(), &ctx).unwrap_err();
        assert!(
            err.to_string().contains("expects 2 args"),
            "want arity error, got: {err}"
        );
    }

    // H1 — Ge/Le hard eval
    #[test]
    fn ge_le_hard_eval() {
        // close >= 3 true on closes [1,2,3]: last close is 3, 3>=3 true
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0]);
        let f = |src: &str| eval(&parse_str(src).unwrap(), &ctx).unwrap();
        assert_eq!(f("close >= 3"), Value::Bool(true));
        assert_eq!(f("5 <= 5"), Value::Bool(true));
        assert_eq!(f("2 >= 3"), Value::Bool(false));
    }

    // M2 — Ge/Le fuzzy eval
    #[test]
    fn ge_le_fuzzy_eval() {
        let ctx = ctx_from_closes(&[1.0]);
        let f = |src: &str| eval_fuzzy(&parse_str(src).unwrap(), &ctx, 0.02).unwrap();
        // 10 >= 10: equal → 0.5
        assert!((f("10 >= 10") - 0.5).abs() < 1e-9);
        // 9.8 <= 10: lhs < rhs → >0.5
        assert!(f("9.8 <= 10") > 0.5);
    }

    // M1 — eval dispatch: ema, rsi, atr, crossover/crossunder
    #[test]
    fn indicator_dispatch_no_error() {
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        // ema(close,3) > 0 → Bool
        assert_eq!(
            eval(&parse_str("ema(close,3) > 0").unwrap(), &ctx).unwrap(),
            Value::Bool(true)
        );
        // rsi(close,3) >= 0 → Bool
        assert_eq!(
            eval(&parse_str("rsi(close,3) >= 0").unwrap(), &ctx).unwrap(),
            Value::Bool(true)
        );
        // atr(2) >= 0 → Bool
        assert_eq!(
            eval(&parse_str("atr(2) >= 0").unwrap(), &ctx).unwrap(),
            Value::Bool(true)
        );
        // crossover or not crossover → always Bool
        match eval(&parse_str("crossover(close, sma(close,3)) or not crossover(close, sma(close,3))").unwrap(), &ctx).unwrap() {
            Value::Bool(_) => {}
            other => panic!("expected Bool, got {other:?}"),
        }
        // crossunder as a boolean combined with or: Bool or Bool → Bool
        match eval(&parse_str("crossunder(close, sma(close,3)) or not crossunder(close, sma(close,3))").unwrap(), &ctx).unwrap() {
            Value::Bool(_) => {}
            other => panic!("expected Bool from crossunder or, got {other:?}"),
        }
    }

    #[test]
    fn aux_identifier_resolves_and_gates() {
        let mut ctx = ctx_from_closes(&[1.0, 2.0, 3.0]);
        ctx.aux.insert("idx".to_string(), crate::features::context::AuxView {
            cols: std::collections::BTreeMap::from([("v".to_string(), vec![10.0, 20.0])]),
        });
        let f = |src: &str, c: &Context| eval(&parse_str(src).unwrap(), c).unwrap();
        // 归约取 last
        assert_eq!(f("aux.idx.v == 20", &ctx), Value::Bool(true));
        assert_eq!(f("aux.idx.v[-1] == 10", &ctx), Value::Bool(true));
        // 缺列/缺表 → Err
        assert!(eval(&parse_str("aux.idx.nope > 0").unwrap(), &ctx).is_err());
        assert!(eval(&parse_str("aux.none.v > 0").unwrap(), &ctx).is_err());
        // 空截断 → NaN → 比较 false（弃权）
        ctx.aux.get_mut("idx").unwrap().cols.insert("v".to_string(), vec![]);
        assert_eq!(f("aux.idx.v > 0", &ctx), Value::Bool(false));
    }

    #[test]
    fn time_identifiers_hour_minute_dow() {
        let ctx = ctx_from_closes(&[1.0]); // t = 2024-01-02 09:45（周二）
        let f = |src: &str| eval(&parse_str(src).unwrap(), &ctx).unwrap();
        assert_eq!(f("hour == 9"), Value::Bool(true));
        assert_eq!(f("minute == 45"), Value::Bool(true));
        assert_eq!(f("dow == 2"), Value::Bool(true));
        assert_eq!(f("dow <= 5"), Value::Bool(true));
        // fuzzy 路径可用（比较经 as_scalar）
        assert!((eval_fuzzy(&parse_str("hour >= 9").unwrap(), &ctx, 0.02).unwrap() - 0.5).abs() < 0.5);
    }

    #[test]
    fn sim_state_identifiers() {
        let mut ctx = ctx_from_closes(&[1.0]);
        let f = |src: &str, c: &Context| eval(&parse_str(src).unwrap(), c).unwrap();
        // 默认（非 sim）：pos=0、bars_held=0、unreal=0；entry_price=NaN → 比较弃权
        assert_eq!(f("pos == 0", &ctx), Value::Bool(true));
        assert_eq!(f("bars_held == 0", &ctx), Value::Bool(true));
        assert_eq!(f("unreal_pnl == 0", &ctx), Value::Bool(true));
        assert_eq!(f("entry_price > 0", &ctx), Value::Bool(false)); // NaN 弃权
        // 注入后可见
        ctx.sim = crate::features::context::SimState { pos: 0.5, entry_price: 10.0, bars_held: 3, unreal_pnl: -0.02 };
        assert_eq!(f("pos > 0 and bars_held >= 3", &ctx), Value::Bool(true));
        assert_eq!(f("unreal_pnl < -0.01 and entry_price == 10", &ctx), Value::Bool(true));
    }
}
