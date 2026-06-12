use crate::dsl::ast::{BinaryOp, Expr, UnaryOp};
use crate::features::context::Context;
use crate::features::indicators;
use crate::{Error, Result};
use chrono::{Datelike, Timelike};

/// 当日尾部连续段：可见窗内 date == t.date() 的末段切片范围。
/// 从窗尾向前扫，遇到日期变化为止（全 Scalar、纯 Context 派生，无前视）。
fn session_range(ctx: &Context) -> std::ops::Range<usize> {
    let bars = &ctx.primary.bars;
    let today = ctx.t.date();
    let mut start = bars.len();
    while start > 0 && bars[start - 1].time.date() == today {
        start -= 1;
    }
    start..bars.len()
}

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
        Expr::Cached(_, e) => eval_fuzzy(e, ctx, scale),
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
            "max_price_since_entry" => Ok(Value::Scalar(ctx.sim.max_price_since_entry)),
            "min_price_since_entry" => Ok(Value::Scalar(ctx.sim.min_price_since_entry)),
            "bars_since_exit" => Ok(Value::Scalar(ctx.sim.bars_since_exit)),
            "last_trip_return" => Ok(Value::Scalar(ctx.sim.last_trip_return)),
            // 日内锚定族：当日尾部连续段（从窗尾向前扫 date==today）；全 Scalar、无前视。
            "bars_today" => {
                let r = session_range(ctx);
                // r 不可能为空（t 本身即窗尾 bar 时间，至少含 1 根）；防御仍给 1.0
                Ok(Value::Scalar(if r.is_empty() { 1.0 } else { r.len() as f64 }))
            }
            "session_open" => {
                let r = session_range(ctx);
                Ok(Value::Scalar(if r.is_empty() {
                    f64::NAN
                } else {
                    ctx.primary.bars[r.start].open
                }))
            }
            "session_high" => {
                let r = session_range(ctx);
                Ok(Value::Scalar(if r.is_empty() {
                    f64::NAN
                } else {
                    ctx.primary.bars[r.clone()]
                        .iter()
                        .map(|b| b.high)
                        .fold(f64::NEG_INFINITY, f64::max)
                }))
            }
            "session_low" => {
                let r = session_range(ctx);
                Ok(Value::Scalar(if r.is_empty() {
                    f64::NAN
                } else {
                    ctx.primary.bars[r.clone()]
                        .iter()
                        .map(|b| b.low)
                        .fold(f64::INFINITY, f64::min)
                }))
            }
            "session_vwap" => {
                let r = session_range(ctx);
                if r.is_empty() {
                    return Ok(Value::Scalar(f64::NAN));
                }
                let (sum_cv, sum_v) = ctx.primary.bars[r.clone()].iter().fold(
                    (0.0_f64, 0.0_f64),
                    |(scv, sv), b| (scv + b.close * b.volume, sv + b.volume),
                );
                // Σv ≤ 0 → NaN 弃权（含全零量 volume 场景）
                Ok(Value::Scalar(if sum_v <= 0.0 { f64::NAN } else { sum_cv / sum_v }))
            }
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
                UnaryOp::Neg => match v {
                    Value::Series(s) => Ok(Value::Series(s.iter().map(|x| -x).collect())),
                    other => Ok(Value::Scalar(-as_scalar(&other)?)),
                },
                UnaryOp::Not => Ok(Value::Bool(!as_bool(&v)?)),
            }
        }
        Expr::Binary(op, l, r) => {
            let lv = eval(l, ctx)?;
            let rv = eval(r, ctx)?;
            Ok(match op {
                BinaryOp::And => Value::Bool(as_bool(&lv)? && as_bool(&rv)?),
                BinaryOp::Or => Value::Bool(as_bool(&lv)? || as_bool(&rv)?),
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => {
                    arith(op, &lv, &rv)?
                }
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
        Expr::Cached(id, inner) => {
            if let Some(v) = ctx.eval_cache.borrow().get(id) {
                return Ok(v.clone());
            }
            let v = eval(inner, ctx)?;
            ctx.eval_cache.borrow_mut().insert(*id, v.clone());
            Ok(v)
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

/// 一元点态提升：Series → 逐位 map；Scalar → 标量。NaN 自然传播（f(NaN)=NaN）。
/// Bool → Err（与 as_scalar 同等拒绝）。
fn pointwise1(v: &Value, f: impl Fn(f64) -> f64) -> Result<Value> {
    match v {
        Value::Scalar(x) => Ok(Value::Scalar(f(*x))),
        Value::Series(s) => Ok(Value::Series(s.iter().map(|&x| f(x)).collect())),
        Value::Bool(_) => Err(Error::Eval("expected number, got bool".into())),
    }
}

/// 二元点态提升：≥1 侧 Series → 尾对齐逐位；双标量 → 标量；NaN 规则由闭包自带。
/// Bool → Err（与 arith 同等拒绝）。
fn pointwise2(a: &Value, b: &Value, f: impl Fn(f64, f64) -> f64) -> Result<Value> {
    match (a, b) {
        (Value::Bool(_), _) | (_, Value::Bool(_)) => {
            Err(Error::Eval("expected number, got bool".into()))
        }
        (Value::Scalar(x), Value::Scalar(y)) => Ok(Value::Scalar(f(*x, *y))),
        _ => {
            let (xs, ys) = tail_align(a, b)?;
            Ok(Value::Series(xs.iter().zip(&ys).map(|(&x, &y)| f(x, y)).collect()))
        }
    }
}

/// 算术逐位提升：≥1 侧 Series → 尾对齐逐位运算返回 Series（末位恒等于旧标量结果）；
/// 双标量 → 标量（形态守则：lint L2 依赖）；Bool → Err（与 as_scalar 同等拒绝）。
fn arith(op: &BinaryOp, lv: &Value, rv: &Value) -> Result<Value> {
    let apply = |a: f64, b: f64| match op {
        BinaryOp::Add => a + b,
        BinaryOp::Sub => a - b,
        BinaryOp::Mul => a * b,
        BinaryOp::Div => a / b,
        _ => unreachable!(),
    };
    match (lv, rv) {
        (Value::Bool(_), _) | (_, Value::Bool(_)) => {
            Err(Error::Eval("expected number, got bool".into()))
        }
        (Value::Scalar(a), Value::Scalar(b)) => Ok(Value::Scalar(apply(*a, *b))),
        _ => {
            let (a, b) = tail_align(lv, rv)?;
            Ok(Value::Series(a.iter().zip(&b).map(|(&x, &y)| apply(x, y)).collect()))
        }
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

/// 把两个 Value 调成等长数值序列对（尾对齐）：双 Series 取右端公共长度；Scalar 广播；Bool → Err。
fn tail_align(a: &Value, b: &Value) -> Result<(Vec<f64>, Vec<f64>)> {
    match (a, b) {
        (Value::Series(x), Value::Series(y)) => {
            let m = x.len().min(y.len());
            Ok((x[x.len() - m..].to_vec(), y[y.len() - m..].to_vec()))
        }
        (Value::Series(x), Value::Scalar(s)) => Ok((x.clone(), vec![*s; x.len()])),
        (Value::Scalar(s), Value::Series(y)) => Ok((vec![*s; y.len()], y.clone())),
        (Value::Scalar(p), Value::Scalar(q)) => Ok((vec![*p], vec![*q])),
        _ => Err(Error::Eval("expected numeric operands in condition".into())),
    }
}

/// 布尔序列求值（count/barssince 的条件臂）：比较 → 逐位（任一侧 NaN → 该位 false），
/// and/or/not → 逐位组合（尾对齐到公共长度），crossover/crossunder → 逐位事件序列，
/// 其余表达式形态 → Err。
/// 注意：此处的 crossover/crossunder 是逐位事件语义，与 eval_call 的标量 Bool 版
///（只看末两位）是同名函数的两种刻意并存的语义——条件序列上下文 vs 普通 when 上下文。
fn eval_bool_series(expr: &Expr, ctx: &Context) -> Result<Vec<bool>> {
    match expr {
        Expr::Binary(op, l, r) => match op {
            BinaryOp::And | BinaryOp::Or => {
                let a = eval_bool_series(l, ctx)?;
                let b = eval_bool_series(r, ctx)?;
                let m = a.len().min(b.len());
                let (a, b) = (&a[a.len() - m..], &b[b.len() - m..]);
                Ok(a.iter()
                    .zip(b)
                    .map(|(&x, &y)| if matches!(op, BinaryOp::And) { x && y } else { x || y })
                    .collect())
            }
            BinaryOp::Gt | BinaryOp::Lt | BinaryOp::Ge | BinaryOp::Le | BinaryOp::Eq | BinaryOp::Ne => {
                let (a, b) = tail_align(&eval(l, ctx)?, &eval(r, ctx)?)?;
                Ok(a.iter()
                    .zip(&b)
                    .map(|(&x, &y)| {
                        if x.is_nan() || y.is_nan() {
                            return false;
                        }
                        match op {
                            BinaryOp::Gt => x > y,
                            BinaryOp::Lt => x < y,
                            BinaryOp::Ge => x >= y,
                            BinaryOp::Le => x <= y,
                            BinaryOp::Eq => x == y,
                            BinaryOp::Ne => x != y,
                            _ => unreachable!(),
                        }
                    })
                    .collect())
            }
            _ => Err(Error::Eval(
                "count/barssince: condition must be a comparison, boolean combination, or crossover/crossunder call".into(),
            )),
        },
        Expr::Unary(UnaryOp::Not, e) => Ok(eval_bool_series(e, ctx)?.into_iter().map(|x| !x).collect()),
        Expr::Cached(_, e) => eval_bool_series(e, ctx),
        Expr::Call(name, args) if name == "crossover" || name == "crossunder" => {
            if args.len() != 2 {
                return Err(Error::Eval(format!("{name} expects 2 args, got {}", args.len())));
            }
            let (a, b) = tail_align(&eval(&args[0], ctx)?, &eval(&args[1], ctx)?)?;
            let over = name == "crossover";
            Ok((0..a.len())
                .map(|j| {
                    if j == 0 {
                        return false;
                    }
                    let (p0, q0, p1, q1) = (a[j - 1], b[j - 1], a[j], b[j]);
                    if p0.is_nan() || q0.is_nan() || p1.is_nan() || q1.is_nan() {
                        return false;
                    }
                    if over { p0 <= q0 && p1 > q1 } else { p0 >= q0 && p1 < q1 }
                })
                .collect())
        }
        _ => Err(Error::Eval(
            "count/barssince: condition must be a comparison, boolean combination, or crossover/crossunder call".into(),
        )),
    }
}

fn eval_call(name: &str, args: &[Expr], ctx: &Context) -> Result<Value> {
    // count/barssince 的条件参数必须按原始 AST 逐位求值，不能走统一参数求值（那会归约成单 Bool）
    match name {
        "count" => {
            if args.len() != 2 {
                return Err(Error::Eval(format!("count expects 2 args, got {}", args.len())));
            }
            let cond = eval_bool_series(&args[0], ctx)?;
            let n = as_usize(&eval(&args[1], ctx)?)?;
            if n == 0 || cond.len() < n {
                return Ok(Value::Scalar(f64::NAN)); // 窗口不足 → 弃权
            }
            return Ok(Value::Scalar(cond[cond.len() - n..].iter().filter(|&&b| b).count() as f64));
        }
        "barssince" => {
            if args.len() != 1 {
                return Err(Error::Eval(format!("barssince expects 1 arg, got {}", args.len())));
            }
            let cond = eval_bool_series(&args[0], ctx)?;
            return Ok(Value::Scalar(match cond.iter().rposition(|&b| b) {
                Some(j) => (cond.len() - 1 - j) as f64,
                None => f64::NAN, // 可见窗口内从未触发 → 弃权
            }));
        }
        "valuewhen" => {
            if !(2..=3).contains(&args.len()) {
                return Err(Error::Eval(format!(
                    "valuewhen expects 2 or 3 args (cond, expr[, occurrence]), got {}",
                    args.len()
                )));
            }
            let cond = eval_bool_series(&args[0], ctx)?;
            let vals = as_series(&eval(&args[1], ctx)?)?;
            let occ = if args.len() == 3 { as_usize(&eval(&args[2], ctx)?)? } else { 0 };
            let m = cond.len().min(vals.len());
            let (cond, vals) = (&cond[cond.len() - m..], &vals[vals.len() - m..]);
            let mut seen = 0usize;
            for j in (0..m).rev() {
                if cond[j] {
                    if seen == occ {
                        return Ok(Value::Scalar(vals[j]));
                    }
                    seen += 1;
                }
            }
            return Ok(Value::Scalar(f64::NAN)); // 触发次数不足 → 弃权
        }
        _ => {}
    }
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
        "slope" => { need(&vals, 2, name)?; Ok(Value::Series(indicators::slope_roll(&as_series(&vals[0])?, as_usize(&vals[1])?))) }
        "ref" => {
            need(&vals, 2, name)?;
            let s = as_series(&vals[0])?;
            let k = as_usize(&vals[1])?;
            let end = s.len().saturating_sub(k);
            Ok(Value::Series(s[..end].to_vec()))
        }
        "highest" => { need(&vals, 2, name)?; Ok(Value::Series(indicators::highest_roll(&as_series(&vals[0])?, as_usize(&vals[1])?))) }
        "lowest"  => { need(&vals, 2, name)?; Ok(Value::Series(indicators::lowest_roll(&as_series(&vals[0])?, as_usize(&vals[1])?))) }
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
        "std"     => { need(&vals, 2, name)?; Ok(Value::Series(indicators::std_roll(&as_series(&vals[0])?, as_usize(&vals[1])?))) }
        "sigmoid" => {
            need(&vals, 1, name)?;
            pointwise1(&vals[0], |x| 1.0 / (1.0 + (-x).exp()))
        }
        "abs" => {
            need(&vals, 1, name)?;
            pointwise1(&vals[0], f64::abs)
        }
        "max" => {
            need(&vals, 2, name)?;
            // f64::max(NaN, x) 返回 x，会吞掉预热弃权 → 显式传播 NaN（逐位同等规则）
            pointwise2(&vals[0], &vals[1], |a, b| {
                if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) }
            })
        }
        "min" => {
            need(&vals, 2, name)?;
            // f64::min(NaN, x) 返回 x，会吞掉预热弃权 → 显式传播 NaN（逐位同等规则）
            pointwise2(&vals[0], &vals[1], |a, b| {
                if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) }
            })
        }
        // 新数学函数（点态提升）
        "log" => {
            need(&vals, 1, name)?;
            // 自然对数；负定义域 → NaN（Rust 原生），恰合弃权语义
            pointwise1(&vals[0], f64::ln)
        }
        "exp" => {
            need(&vals, 1, name)?;
            pointwise1(&vals[0], f64::exp)
        }
        "sqrt" => {
            need(&vals, 1, name)?;
            // 负数 → NaN（Rust 原生），恰合弃权语义
            pointwise1(&vals[0], f64::sqrt)
        }
        "floor" => {
            need(&vals, 1, name)?;
            pointwise1(&vals[0], f64::floor)
        }
        "sign" => {
            need(&vals, 1, name)?;
            // 数学惯例：sign(0)=0（Rust signum(0.0)=1.0 不合惯例，故自写）；NaN→NaN 自然成立
            pointwise1(&vals[0], |x| if x == 0.0 { 0.0 } else { x.signum() })
        }
        "pow" => {
            need(&vals, 2, name)?;
            pointwise2(&vals[0], &vals[1], f64::powf)
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
        Context { t, primary: Window { bars: bars.clone() }, context: Window { bars }, news: None, aux: std::collections::BTreeMap::new(), sim: crate::features::context::SimState::default(), eval_cache: Default::default() }
    }

    #[test]
    fn cached_expr_memoizes_per_context() {
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0]);
        let e = Expr::Cached(7, Box::new(parse_str("sma(close, 2)").unwrap()));
        // 首次求值：真算，并写入缓存槽
        let v1 = eval(&e, &ctx).unwrap();
        assert!(matches!(v1, Value::Series(_)));
        assert!(ctx.eval_cache.borrow().contains_key(&7));
        // 改写缓存槽为哨兵 → 第二次求值必须命中缓存（返回哨兵而非重算）
        ctx.eval_cache.borrow_mut().insert(7, Value::Scalar(42.0));
        assert_eq!(eval(&e, &ctx).unwrap(), Value::Scalar(42.0));
        // 不同槽位互不串扰
        let e2 = Expr::Cached(8, Box::new(parse_str("close").unwrap()));
        assert!(matches!(eval(&e2, &ctx).unwrap(), Value::Series(_)));
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
        // Task1 后 slope 返回 Series；标量上下文取末位，等价不变
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        let e = parse_str("slope(close,5)").unwrap();
        // 直接 eval 返回 Series，末位值为 1.0（等差斜率）
        match eval(&e, &ctx).unwrap() {
            Value::Series(s) => assert!((s.last().copied().unwrap() - 1.0).abs() < 1e-9),
            other => panic!("expected Series, got {other:?}"),
        }
        // 标量上下文（比较）仍正常工作
        assert_eq!(eval(&parse_str("slope(close,5) > 0.9").unwrap(), &ctx).unwrap(), Value::Bool(true));
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
        // Task1 后 std 返回 Series；末位值仍是 sqrt(2)（总体标准差，等价不变）
        match eval(&parse_str("std(close,5)").unwrap(), &ctx).unwrap() {
            Value::Series(s) => assert!((s.last().copied().unwrap() - 2.0_f64.sqrt()).abs() < 1e-9),
            other => panic!("expected Series, got {other:?}"),
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
    fn abs_min_max_eval() {
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        let f = |src: &str| eval(&parse_str(src).unwrap(), &ctx).unwrap();
        assert_eq!(f("abs(0 - 3) == 3"), Value::Bool(true));
        assert_eq!(f("abs(close - 10) == 5"), Value::Bool(true));
        assert_eq!(f("max(2, 3) == 3"), Value::Bool(true));
        assert_eq!(f("min(2, 3) == 2"), Value::Bool(true));
        // 序列参数经 as_scalar 归约取末元素
        assert_eq!(f("max(close, 4.5) == 5"), Value::Bool(true));
        // NaN 传播：预热期 sma 为 NaN → max/min 必须返回 NaN（弃权），不得吃掉 NaN
        assert_eq!(f("max(sma(close, 10), 1) > 0"), Value::Bool(false));
        assert_eq!(f("min(sma(close, 10), 1) < 99"), Value::Bool(false));
        // abs 对 NaN 输入也应弃权（预热期 sma 为 NaN）
        assert_eq!(f("abs(sma(close, 10)) > 0"), Value::Bool(false));
        // 错参数量
        assert!(eval(&parse_str("abs(1, 2)").unwrap(), &ctx).is_err());
        assert!(eval(&parse_str("max(1)").unwrap(), &ctx).is_err());
    }

    #[test]
    fn count_over_bool_series() {
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        let f = |src: &str| eval(&parse_str(src).unwrap(), &ctx).unwrap();
        // 末 3 位 [3>3,4>3,5>3] = [F,T,T] → 2
        assert_eq!(f("count(close > 3, 3) == 2"), Value::Bool(true));
        // 预热 NaN 逐位弃权：sma(close,3)=[N,N,2,3,4]，close>sma 逐位 [F,F,T,T,T] → 3
        assert_eq!(f("count(close > sma(close,3), 5) == 3"), Value::Bool(true));
        // and 逐位组合：close>2 且 close<5 → [F,F,T,T,F] 末 5 位 → 2
        assert_eq!(f("count(close > 2 and close < 5, 5) == 2"), Value::Bool(true));
        // not 逐位
        assert_eq!(f("count(not (close > 3), 5) == 3"), Value::Bool(true));
        // 尾对齐：ref(close,1)=[1,2,3,4] 与标量 2 广播 → [F,F,T,T]，n=4 → 2
        assert_eq!(f("count(ref(close,1) > 2, 4) == 2"), Value::Bool(true));
        // 序列不足 n → NaN 弃权（所有比较 false）
        assert_eq!(f("count(close > 0, 99) > 0"), Value::Bool(false));
        assert_eq!(f("count(close > 0, 99) == count(close > 0, 99)"), Value::Bool(false));
        // Task1 滚动统一后：highest/lowest 在逐位条件中是滚动序列（不再是双标量弃权）。
        // fixture [1..5]：hi3=[1,2,3,4,5] vs lo3=[1,1,1,2,3] → 逐位 > = [F,T,T,T,T] → count=4
        assert_eq!(f("count(highest(close,3) > lowest(close,3), 5) == 4"), Value::Bool(true));
        // 条件不是布尔表达式 → Err
        assert!(eval(&parse_str("count(close, 3)").unwrap(), &ctx).is_err());
        // 错参数量 → Err
        assert!(eval(&parse_str("count(close > 0)").unwrap(), &ctx).is_err());
    }

    #[test]
    fn count_works_in_fuzzy_strength() {
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        // count(close>3,3)=2，两侧相等 → 模糊真值 0.5
        let v = eval_fuzzy(&parse_str("count(close > 3, 3) >= 2").unwrap(), &ctx, 0.02).unwrap();
        assert!((v - 0.5).abs() < 1e-9);
        // barssince 返回 Scalar，fuzzy 路径同样可用：close<2.5 → [T,T,F,F,F] → barssince=3，两侧相等 → 0.5
        let v2 = eval_fuzzy(&parse_str("barssince(close < 2.5) >= 3").unwrap(), &ctx, 0.02).unwrap();
        assert!((v2 - 0.5).abs() < 1e-9);
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
        ctx.sim = crate::features::context::SimState { pos: 0.5, entry_price: 10.0, bars_held: 3, unreal_pnl: -0.02, ..crate::features::context::SimState::default() };
        assert_eq!(f("pos > 0 and bars_held >= 3", &ctx), Value::Bool(true));
        assert_eq!(f("unreal_pnl < -0.01 and entry_price == 10", &ctx), Value::Bool(true));
    }

    #[test]
    fn count_crossover_events() {
        // closes 围绕 2.5 来回穿越：上穿发生在 idx 1、3、5
        let ctx = ctx_from_closes(&[1.0, 3.0, 2.0, 3.0, 2.0, 3.0]);
        let f = |src: &str| eval(&parse_str(src).unwrap(), &ctx).unwrap();
        assert_eq!(f("count(crossover(close, 2.5), 6) == 3"), Value::Bool(true));
        assert_eq!(f("count(crossunder(close, 2.5), 6) == 2"), Value::Bool(true));
        // 与逐位 and 组合
        assert_eq!(f("count(crossover(close, 2.5) and close > 0, 6) == 3"), Value::Bool(true));
        // barssince + crossover：最近一次上穿在 idx5（当前 bar）→ 0
        assert_eq!(f("barssince(crossover(close, 2.5)) == 0"), Value::Bool(true));
        // 预热 NaN 段不产生事件：sma(close,3)=[N,N,2.0,2.67,2.33,2.67]，上穿仅 idx3、idx5 → 2
        assert_eq!(f("count(crossover(sma(close,3), 2.5), 6) == 2"), Value::Bool(true));
    }

    /// 两天数据工厂（day1: 3 根 + day2: 2 根），t = day2 第二根（index 4）。
    /// 每根 bar 有真实不同日期，用于日内锚定族测试。
    fn ctx_two_days() -> Context {
        use chrono::NaiveDate;
        let d1 = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
        let d2 = NaiveDate::from_ymd_opt(2024, 1, 3).unwrap();
        let bars = vec![
            // day1: 3 根
            Bar {
                time: d1.and_hms_opt(10, 0, 0).unwrap(),
                open: 10.0, high: 10.5, low: 9.8, close: 10.2, volume: 100.0,
            },
            Bar {
                time: d1.and_hms_opt(10, 15, 0).unwrap(),
                open: 10.2, high: 10.7, low: 10.0, close: 10.4, volume: 150.0,
            },
            Bar {
                time: d1.and_hms_opt(10, 30, 0).unwrap(),
                open: 10.4, high: 10.9, low: 10.2, close: 10.6, volume: 120.0,
            },
            // day2: 2 根
            Bar {
                time: d2.and_hms_opt(10, 0, 0).unwrap(),
                open: 10.6, high: 11.0, low: 10.4, close: 10.8, volume: 80.0,
            },
            Bar {
                time: d2.and_hms_opt(10, 15, 0).unwrap(),
                open: 10.8, high: 11.2, low: 10.6, close: 11.0, volume: 90.0,
            },
        ];
        let t = bars[4].time;
        Context {
            t,
            primary: Window { bars: bars.clone() },
            context: Window { bars },
            news: None,
            aux: std::collections::BTreeMap::new(),
            sim: crate::features::context::SimState::default(),
            eval_cache: Default::default(),
        }
    }

    /// 两天数据工厂（volume=0），用于 session_vwap NaN 弃权测试。
    fn ctx_two_days_zero_vol() -> Context {
        use chrono::NaiveDate;
        let d1 = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
        let d2 = NaiveDate::from_ymd_opt(2024, 1, 3).unwrap();
        let bars = vec![
            Bar {
                time: d1.and_hms_opt(10, 0, 0).unwrap(),
                open: 10.0, high: 10.5, low: 9.8, close: 10.2, volume: 0.0,
            },
            Bar {
                time: d2.and_hms_opt(10, 0, 0).unwrap(),
                open: 10.6, high: 11.0, low: 10.4, close: 10.8, volume: 0.0,
            },
        ];
        let t = bars[1].time;
        Context {
            t,
            primary: Window { bars: bars.clone() },
            context: Window { bars },
            news: None,
            aux: std::collections::BTreeMap::new(),
            sim: crate::features::context::SimState::default(),
            eval_cache: Default::default(),
        }
    }

    /// 单日首根退化工厂（t = day1 首根，bars_today == 1）。
    fn ctx_single_bar_first() -> Context {
        use chrono::NaiveDate;
        let d1 = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
        let d2 = NaiveDate::from_ymd_opt(2024, 1, 3).unwrap();
        // 窗口内只有 day1 的两根 + day2 的首根，t = day2 首根
        let bars = vec![
            Bar {
                time: d1.and_hms_opt(10, 0, 0).unwrap(),
                open: 10.0, high: 10.5, low: 9.8, close: 10.2, volume: 100.0,
            },
            Bar {
                time: d1.and_hms_opt(10, 15, 0).unwrap(),
                open: 10.2, high: 10.7, low: 10.0, close: 10.4, volume: 150.0,
            },
            Bar {
                time: d2.and_hms_opt(10, 0, 0).unwrap(),
                open: 10.6, high: 11.0, low: 10.4, close: 10.8, volume: 80.0,
            },
        ];
        let t = bars[2].time; // day2 首根 = 单日首根退化
        Context {
            t,
            primary: Window { bars: bars.clone() },
            context: Window { bars },
            news: None,
            aux: std::collections::BTreeMap::new(),
            sim: crate::features::context::SimState::default(),
            eval_cache: Default::default(),
        }
    }

    #[test]
    fn session_anchors_two_day_window() {
        // 两天数据：day1 3 根 + day2 2 根，t = day2 第二根（index 4）
        // 手算：
        //   bars_today = 2（day2 两根）
        //   session_open = day2 首根 open = 10.6
        //   session_high = max(day2-bar0.high=11.0, day2-bar1.high=11.2) = 11.2
        //   session_low  = min(day2-bar0.low=10.4,  day2-bar1.low=10.6)  = 10.4
        //   session_vwap = (10.8*80 + 11.0*90)/(80+90) = (864+990)/170 = 1854/170
        let ctx = ctx_two_days();
        let f = |src: &str| eval(&parse_str(src).unwrap(), &ctx).unwrap();

        assert_eq!(f("bars_today == 2"), Value::Bool(true));
        assert_eq!(f("session_open == 10.6"), Value::Bool(true));
        assert_eq!(f("session_high == 11.2"), Value::Bool(true));
        assert_eq!(f("session_low == 10.4"), Value::Bool(true));

        // session_vwap 手算: 1854/170
        let expected_vwap = 1854.0_f64 / 170.0;
        match f("session_vwap") {
            Value::Scalar(v) => assert!((v - expected_vwap).abs() < 1e-9,
                "session_vwap: got {v}, expected {expected_vwap}"),
            other => panic!("expected Scalar, got {other:?}"),
        }
        // 全部标量形态
        assert!(matches!(f("bars_today"), Value::Scalar(_)));
        assert!(matches!(f("session_open"), Value::Scalar(_)));
        assert!(matches!(f("session_high"), Value::Scalar(_)));
        assert!(matches!(f("session_low"), Value::Scalar(_)));
        assert!(matches!(f("session_vwap"), Value::Scalar(_)));
    }

    #[test]
    fn session_anchors_single_bar_degeneration() {
        // 单日首根退化：t = day2 首根 → bars_today == 1
        let ctx = ctx_single_bar_first();
        let f = |src: &str| eval(&parse_str(src).unwrap(), &ctx).unwrap();
        assert_eq!(f("bars_today == 1"), Value::Bool(true));
        // session_open = day2-bar0.open = 10.6
        assert_eq!(f("session_open == 10.6"), Value::Bool(true));
        // session_high = session_low = 单根的 high/low
        assert_eq!(f("session_high == 11.0"), Value::Bool(true));
        assert_eq!(f("session_low == 10.4"), Value::Bool(true));
    }

    #[test]
    fn session_vwap_zero_volume_abstains() {
        // volume 全 0 → Σv=0 → NaN → 比较弃权
        let ctx = ctx_two_days_zero_vol();
        let f = |src: &str| eval(&parse_str(src).unwrap(), &ctx).unwrap();
        // NaN 的任何比较返回 false（弃权）
        assert_eq!(f("session_vwap > 0"), Value::Bool(false));
        assert_eq!(f("session_vwap == session_vwap"), Value::Bool(false));
    }

    #[test]
    fn position_extreme_identifiers() {
        let mut ctx = ctx_from_closes(&[10.4]);
        let f = |src: &str, c: &Context| eval(&parse_str(src).unwrap(), c).unwrap();
        // 空仓默认 NaN → 弃权
        assert_eq!(f("max_price_since_entry > 0", &ctx), Value::Bool(false));
        assert_eq!(f("min_price_since_entry > 0", &ctx), Value::Bool(false));
        // 注入后可见：Chandelier 形态条件可表达
        ctx.sim = crate::features::context::SimState {
            pos: 1.0,
            entry_price: 10.0,
            bars_held: 3,
            unreal_pnl: 0.04,
            max_price_since_entry: 11.0,
            min_price_since_entry: 9.9,
            ..crate::features::context::SimState::default()
        };
        assert_eq!(f("max_price_since_entry == 11", &ctx), Value::Bool(true));
        assert_eq!(f("min_price_since_entry == 9.9", &ctx), Value::Bool(true)); // 判别 max/min 不可接反
        assert_eq!(f("close < max_price_since_entry - 0.5", &ctx), Value::Bool(true));
        // MFE 推导：(11/10 - 1) = 0.1
        assert_eq!(f("max_price_since_entry / entry_price - 1 > 0.09", &ctx), Value::Bool(true));
    }

    #[test]
    fn barssince_last_true_distance() {
        let ctx = ctx_from_closes(&[1.0, 5.0, 2.0, 3.0, 4.0]);
        let f = |src: &str| eval(&parse_str(src).unwrap(), &ctx).unwrap();
        // close>4 仅在 idx1（5.0）为 true → 距末位 3 根
        assert_eq!(f("barssince(close > 4) == 3"), Value::Bool(true));
        // 当前 bar 即 true → 0
        assert_eq!(f("barssince(close > 3.5) == 0"), Value::Bool(true));
        // 多次 true 且最近一次不在尾部：close<2.5 → [T,F,T,F,F]，最近 true 在 idx2 → 距离 2
        // （若误用 position 从头找会得 4——本断言锁定 rposition 语义）
        assert_eq!(f("barssince(close < 2.5) == 2"), Value::Bool(true));
        // 从未 true → NaN 弃权
        assert_eq!(f("barssince(close > 99) >= 0"), Value::Bool(false));
        // 非布尔条件 / 错参数量 → Err
        assert!(eval(&parse_str("barssince(close)").unwrap(), &ctx).is_err());
        assert!(eval(&parse_str("barssince(close > 0, 1)").unwrap(), &ctx).is_err());
    }

    #[test]
    fn valuewhen_anchors_event_values() {
        // closes [1,2,3,4,3]：crossover(close,2.5) 事件仅在 idx2（2→3 上穿）
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0, 4.0, 3.0]);
        let f = |src: &str| eval(&parse_str(src).unwrap(), &ctx).unwrap();
        // 最近一次上穿时的 close = 3
        assert_eq!(f("valuewhen(crossover(close, 2.5), close) == 3"), Value::Bool(true));
        // occurrence=1：再往前一次——不存在 → NaN 弃权（比较恒 false）
        assert_eq!(f("valuewhen(crossover(close, 2.5), close, 1) > 0"), Value::Bool(false));
        // 从未触发 → NaN 弃权
        assert_eq!(f("valuewhen(close > 99, close) > 0"), Value::Bool(false));
        // 条件与取值序列尾对齐：ref(close,1)=[1,2,3,4]，事件位取移后值 = 2
        assert_eq!(f("valuewhen(crossover(close, 2.5), ref(close, 1)) == 2"), Value::Bool(true));
        // 锚定惯用法：closes=[1,2,3,4,3]，highest_roll(close,2)=[1,2,3,4,4]
        // close >= highest_roll 逐位 = [T,T,T,T,F]，最近真位 = idx3，close[3] = 4
        assert_eq!(f("valuewhen(close >= highest(close, 2), close) == 4"), Value::Bool(true));
        // 参数个数校验
        assert!(eval(&parse_str("valuewhen(close > 0)").unwrap(), &ctx).is_err());
        assert!(eval(&parse_str("valuewhen(close > 0, close, 1, 2)").unwrap(), &ctx).is_err());
    }

    #[test]
    fn rolling_forms_unlock_elementwise_conditions() {
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        let f = |src: &str| eval(&parse_str(src).unwrap(), &ctx).unwrap();
        // highest_roll(close,2)=[1,2,3,4,5]（宽容头），close >= 它 ⇒ 逐位全真（创新高序列）
        assert_eq!(f("count(close >= highest(close, 2), 5) == 5"), Value::Bool(true));
        // barssince + 滚动 highest：最近一次创 2 根新高就在当前 bar
        assert_eq!(f("barssince(close >= highest(close, 2)) == 0"), Value::Bool(true));
        // 标量上下文不变：highest(close,3) 仍按末位取值 = 5
        assert_eq!(f("highest(close, 3) == 5"), Value::Bool(true));
        assert_eq!(f("slope(close, 5) > 0.9 and slope(close, 5) < 1.1"), Value::Bool(true));
    }

    /// 提升定理锁：任意混合算术表达式在标量上下文的值与提升前完全一致。
    /// 每个用例：手算旧标量语义的期望值，断言 as_scalar(eval(expr)) 逐 bits 相等。
    #[test]
    fn arithmetic_lift_scalar_context_equivalence() {
        // ctx_from_closes: open=close=high=low=c, volume=1.0（常数）
        // closes [1,2,3,4,5]
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        let f = |src: &str| {
            let v = eval(&parse_str(src).unwrap(), &ctx).unwrap();
            match v {
                Value::Scalar(x) => x,
                Value::Series(s) => *s.last().unwrap(),
                Value::Bool(_) => panic!("unexpected bool"),
            }
        };
        // Series∘Series：末位 = 5.0 * 1.0 = 5.0（volume 常数 1.0）
        // f("volume") → as_scalar(Series([1,1,1,1,1])) = 1.0
        assert_eq!(f("close * volume").to_bits(), (5.0_f64 * f("volume")).to_bits());
        // Series∘Scalar 广播：末位 = 5 - 1 = 4.0
        assert_eq!(f("close - 1"), 4.0);
        // 函数序列参与：sma(close,2).last = (4+5)/2 = 4.5 → close - sma = 5 - 4.5 = 0.5
        assert_eq!(f("close - sma(close, 2)"), 0.5);
        // 嵌套：highest(close,3).last=5, lowest(close,3).last=3 → (5-3)/5 = 0.4
        assert_eq!(f("(highest(close,3) - lowest(close,3)) / close"), 0.4);
        // 一元负号（Scalar 路径仍为 Scalar）：0 是 Scalar，close 是 Series → Series，末位 0-5=-5
        assert_eq!(f("0 - close"), -5.0);
        // 双标量仍是 Scalar 形态（守则锁——L2 依赖）
        assert!(matches!(
            eval(&parse_str("pos * 2 + 1").unwrap(), &ctx).unwrap(),
            Value::Scalar(_)
        ));
        // Bool 进算术仍 Err（两个方向都锁）
        assert!(eval(&parse_str("(close > 1) + 1").unwrap(), &ctx).is_err());
        assert!(eval(&parse_str("1 + (close > 1)").unwrap(), &ctx).is_err());
    }

    /// 派生序列解锁：算术结果可进窗口函数与逐位条件（phase-2 的存在意义）。
    #[test]
    fn derived_series_feed_windows_and_conditions() {
        // ctx_from_closes: open=close=high=low=c, volume=1.0（常数）
        // closes [1,2,3,4,5]
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        let f = |src: &str| eval(&parse_str(src).unwrap(), &ctx).unwrap();
        // 滚动 VWAP 恒等：volume=1.0 常数时 sma(close*1,3)/sma(1,3) = sma(close,3)/1 = sma(close,3)
        // 两侧 Series，== 取末位比较：sma(close,3).last=(3+4+5)/3=4.0；4.0==4.0 → true
        assert_eq!(
            f("sma(close * volume, 3) / sma(volume, 3) == sma(close, 3)"),
            Value::Bool(true)
        );
        // 派生序列进逐位条件：open==close（工厂），改用 close - 1 > 0
        // close-1=[0,1,2,3,4]（Series），>0 逐位=[F,T,T,T,T]，count 末 4 位=[T,T,T,T]=4；4==4→true
        assert_eq!(f("count(close - 1 > 0, 4) == 4"), Value::Bool(true));
        // 派生序列进 ref：close*2=[2,4,6,8,10]，ref(...,1) 去末 1 个 → [2,4,6,8]，末位=8；8==8→true
        assert_eq!(f("ref(close * 2, 1) == 8"), Value::Bool(true));
        // 逐位 NaN 传播：sma(close,3)=[NaN,NaN,2,3,4]，close-sma=[NaN,NaN,1,1,1]
        // 0-99 两标量 → Scalar(-99)；NaN>-99→false，其余 1>-99→true
        // bools=[F,F,T,T,T]，count 末 5=3；3==3→true
        assert_eq!(f("count(close - sma(close,3) > 0 - 99, 5) == 3"), Value::Bool(true));
    }

    /// T2：点态函数提升（abs/min/max/sigmoid）+ 数学补全（log/exp/sqrt/floor/sign/pow）
    /// ctx_from_closes: open=close=high=low=c, volume=1.0（常数）; closes [1,2,3,4,5]
    #[test]
    fn pointwise_fns_lift_and_new_math() {
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        let f = |src: &str| eval(&parse_str(src).unwrap(), &ctx).unwrap();

        // abs 提升：abs(close - 3) 逐位 [|1-3|,|2-3|,|3-3|,|4-3|,|5-3|] = [2,1,0,1,2]
        // >0 的位：[T,T,F,T,T] → count 末 5 = 4
        assert_eq!(f("count(abs(close - 3) > 0, 5) == 4"), Value::Bool(true));

        // abs 全标量仍 Scalar 形态：pos=0（Scalar），abs(pos-1)=abs(-1)=1.0 → Scalar
        assert!(matches!(f("abs(pos - 1)"), Value::Scalar(_)));

        // min 提升 + 广播：min(close, 3) 逐位 [min(1,3),min(2,3),min(3,3),min(4,3),min(5,3)]
        // = [1,2,3,3,3]；==3 逐位 [F,F,T,T,T] → count=3
        assert_eq!(f("count(min(close, 3) == 3, 5) == 3"), Value::Bool(true));

        // max 提升 + 广播：max(close, 3) 逐位 [3,3,3,4,5]；==3 逐位 [T,T,T,F,F] → count=3
        assert_eq!(f("count(max(close, 3) == 3, 5) == 3"), Value::Bool(true));

        // min/max 双标量仍 Scalar（weight 表达式回归保障）
        assert!(matches!(f("min(1, pos + 0.25)"), Value::Scalar(_)));
        assert!(matches!(f("max(1, pos + 0.25)"), Value::Scalar(_)));

        // 新函数标量形态：floor
        // floor(2.9)=2；2==2→true
        assert_eq!(f("floor(2.9) == 2"), Value::Bool(true));

        // sign：数学惯例 sign(0)=0，sign(-3)=-1；(0-3)=-3，sign(-3)=-1；(0-1)=-1；-1==-1→true
        assert_eq!(f("sign(0 - 3) == 0 - 1"), Value::Bool(true));
        // sign(0)=0（数学惯例，Rust signum(0.0)=1.0 不同）
        assert_eq!(f("sign(0) == 0"), Value::Bool(true));

        // sqrt：sqrt(9)=3
        assert_eq!(f("sqrt(9) == 3"), Value::Bool(true));

        // pow：pow(2,10)=1024
        assert_eq!(f("pow(2, 10) == 1024"), Value::Bool(true));

        // log/exp：log(exp(2))≈2
        assert_eq!(f("log(exp(2)) > 1.999 and log(exp(2)) < 2.001"), Value::Bool(true));

        // 负定义域 → NaN → 弃权（比较恒 false）
        // log(0-1)=log(-1)=NaN；NaN>(0-99)→false
        assert_eq!(f("log(0 - 1) > 0 - 99"), Value::Bool(false));
        // sqrt(0-1)=sqrt(-1)=NaN；NaN>(0-99)→false
        assert_eq!(f("sqrt(0 - 1) > 0 - 99"), Value::Bool(false));

        // 序列提升：log(close)=[ln1,ln2,ln3,ln4,ln5]=[0,ln2,ln3,ln4,ln5]
        // log(close)-log(ref(close,1))：ref(close,1)=[1,2,3,4]（末截去1），
        // log([2,3,4,5])-log([1,2,3,4])=[ln2,ln3,ln4,ln5]-[0,ln2,ln3,ln4]
        // = [ln2-0,ln3-ln2,ln4-ln3,ln5-ln4] → 均 >0（对数递增）
        // count 末 4 = 4；4==4→true
        assert_eq!(
            f("count(log(close) - log(ref(close,1)) > 0, 4) == 4"),
            Value::Bool(true)
        );

        // min/max 的 NaN 显式传播在逐位下保持：
        // sma(close,3)=[NaN,NaN,2,3,4]；min(close, sma)=[NaN,NaN,min(3,2),min(4,3),min(5,4)]
        // = [NaN,NaN,2,3,4]；>0 逐位=[F,F,T,T,T]（NaN→false）→ count 末 5=3
        assert_eq!(f("count(min(close, sma(close,3)) > 0, 5) == 3"), Value::Bool(true));

        // sigmoid 提升（全标量回归）：sigmoid(0)=0.5
        match f("sigmoid(0)") {
            Value::Scalar(x) => assert!((x - 0.5).abs() < 1e-9),
            o => panic!("expected Scalar from sigmoid(0), got {o:?}"),
        }
    }
}
