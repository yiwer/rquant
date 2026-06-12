//! 加载期 lint：检出"语法合法但语义必然空转/恒假"的条件写法，eprintln 告警不阻断。
//! 规则随 DSL 形态表演进——形态推断表（expr_shape）必须与 eval.rs 实际返回形态同步。

use crate::dsl::ast::{BinaryOp, Expr, UnaryOp};
use crate::tree::loader::{Node, Strength, Tree, Weight};

/// 表达式形态：与 eval.rs 各臂返回的 Value 形态一一对应。
#[derive(Debug, Clone, Copy, PartialEq)]
enum Shape {
    Series,
    Scalar,
}

/// 返回 Series 的内置函数（Task1 后含 highest/lowest/std/slope）。与 eval.rs 同步维护。
pub(super) const SERIES_FNS: [&str; 13] = [
    "sma", "ema", "wma", "rsi", "atr", "ref",
    "macd_line", "macd_signal", "macd_hist",
    "highest", "lowest", "std", "slope",
];

/// 点态提升函数：形态 = 实参形态的并（任一 Series → Series）。与 eval.rs pointwise 同步维护。
pub(super) const POINTWISE_FNS: [&str; 10] = [
    "abs", "min", "max", "sigmoid", "log", "exp", "sqrt", "floor", "sign", "pow",
];

fn expr_shape(e: &Expr) -> Shape {
    match e {
        Expr::Number(_) => Shape::Scalar,
        Expr::Ident(name) => match name.as_str() {
            "hour" | "minute" | "dow" | "pos" | "entry_price" | "bars_held"
            | "unreal_pnl" | "max_price_since_entry" | "min_price_since_entry" => Shape::Scalar,
            _ => Shape::Series, // close/open/high/low/volume/aux.*/ctx.*
        },
        Expr::Index(..) => Shape::Scalar,
        // phase-2：一元负号随内层形态；Not 实为 Bool——两值 Shape 下并入 Scalar（有意为之，不影响 cond_len_class 正确性）
        Expr::Unary(op, inner) => match op {
            UnaryOp::Neg => expr_shape(inner),
            _ => Shape::Scalar,
        },
        // phase-2：算术随两侧形态并；比较/逻辑仍归 Scalar（Bool）
        Expr::Binary(op, l, r) => match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => {
                if expr_shape(l) == Shape::Series || expr_shape(r) == Shape::Series {
                    Shape::Series
                } else {
                    Shape::Scalar
                }
            }
            _ => Shape::Scalar,
        },
        Expr::Call(name, args) => {
            if SERIES_FNS.contains(&name.as_str()) {
                Shape::Series
            } else if POINTWISE_FNS.contains(&name.as_str()) {
                if args.iter().any(|a| expr_shape(a) == Shape::Series) {
                    Shape::Series
                } else {
                    Shape::Scalar
                }
            } else {
                Shape::Scalar // count/barssince/valuewhen 等
            }
        }
        // Cached(slot_id, inner_expr) — slot id 在前，内层表达式在后
        Expr::Cached(_, inner) => expr_shape(inner),
    }
}

/// 逐位条件的"长度类"：One = 必然塌缩到长度 1（count n>1/barssince/valuewhen 恒弃权）。
#[derive(Debug, Clone, Copy, PartialEq)]
enum LenClass {
    One,
    Many,
}

fn cond_len_class(e: &Expr) -> LenClass {
    match e {
        Expr::Binary(op, l, r) => match op {
            BinaryOp::And | BinaryOp::Or => {
                // 逐位组合尾对齐取公共长度：任一侧塌缩则整体塌缩
                if cond_len_class(l) == LenClass::One || cond_len_class(r) == LenClass::One {
                    LenClass::One
                } else {
                    LenClass::Many
                }
            }
            _ => {
                // 比较：至少一侧 Series 形才有逐位长度
                if expr_shape(l) == Shape::Series || expr_shape(r) == Shape::Series {
                    LenClass::Many
                } else {
                    LenClass::One
                }
            }
        },
        Expr::Unary(_, inner) => cond_len_class(inner), // not
        Expr::Call(name, args) if name == "crossover" || name == "crossunder" => {
            if args.iter().any(|a| expr_shape(a) == Shape::Series) {
                LenClass::Many
            } else {
                LenClass::One
            }
        }
        // Cached(slot_id, inner_expr) — 透传内层
        Expr::Cached(_, inner) => cond_len_class(inner),
        _ => LenClass::One, // 其余形态本就会被 eval 拒绝；保守归 One 不告警双份
    }
}

/// 裸价序列标识符（无 ref/索引移位）——L1 恒假陷阱的构成要件。
fn is_bare_price_ident(e: &Expr) -> Option<&str> {
    match e {
        Expr::Ident(n) if matches!(n.as_str(), "close" | "open" | "high" | "low") => {
            Some(n.as_str())
        }
        // Cached(slot_id, inner_expr) — 透传内层
        Expr::Cached(_, inner) => is_bare_price_ident(inner),
        _ => None,
    }
}

/// e 是否为 highest/lowest(裸价序列, _) 调用（首参未经 ref/索引移位）。
fn bare_window_call(e: &Expr, fname: &str) -> bool {
    match e {
        Expr::Call(n, args) if n == fname && !args.is_empty() => {
            is_bare_price_ident(&args[0]).is_some()
        }
        // Cached(slot_id, inner_expr) — 透传内层
        Expr::Cached(_, inner) => bare_window_call(inner, fname),
        _ => false,
    }
}

/// L1：`X > highest(Y, n)`（及镜像/换序）——窗口含当前 bar，条件恒假。
///
/// 仅在标量求值语境中适用：count/barssince/valuewhen 的条件参数是逐位求值的，
/// 其中 highest/lowest 变成滚动 Series（Task1），比较合法——不递归进入这些函数的条件臂。
fn l1_check(e: &Expr, where_: &str, out: &mut Vec<String>) {
    if let Expr::Binary(op, l, r) = e {
        let hit = match op {
            // 仅严格比较：close >= highest(...) 在"收于窗口极值"时可满足，不是陷阱
            BinaryOp::Gt => {
                (is_bare_price_ident(l).is_some() && bare_window_call(r, "highest"))
                    || (bare_window_call(l, "lowest") && is_bare_price_ident(r).is_some())
            }
            // 仅严格比较：close <= lowest(...) 在"收于窗口极值"时可满足，不是陷阱
            BinaryOp::Lt => {
                (is_bare_price_ident(l).is_some() && bare_window_call(r, "lowest"))
                    || (bare_window_call(l, "highest") && is_bare_price_ident(r).is_some())
            }
            _ => false,
        };
        if hit {
            out.push(format!(
                "{where_}: 突破条件恒假——highest/lowest 窗口含当前 bar；\
                 表达\"前 N 根高/低点\"请先 ref(series, 1) 移窗（docs/dsl-reference.md A1 陷阱）"
            ));
        }
    }
    // 逐位上下文的恒假陷阱（close[j] > highest(滚动窗[j])）同样需要检出——统一递归全部子表达式。
    walk_children(e, &mut |c| l1_check(c, where_, out));
}

/// L2：count/barssince/valuewhen 的条件长度类为 One → 必然弃权空转。
fn l2_check(e: &Expr, where_: &str, out: &mut Vec<String>) {
    if let Expr::Call(name, args) = e
        && matches!(name.as_str(), "count" | "barssince" | "valuewhen")
        && !args.is_empty()
        && cond_len_class(&args[0]) == LenClass::One
    {
        out.push(format!(
            "{where_}: {name}(...) 条件序列长度恒为 1（两侧均无序列形，或 and/or 任一臂退化）——将恒弃权空转；至少一侧需要序列（close/ema(...)/ref(...) 等）"
        ));
    }
    walk_children(e, &mut |c| l2_check(c, where_, out));
}

fn walk_children(e: &Expr, f: &mut impl FnMut(&Expr)) {
    match e {
        Expr::Binary(_, l, r) => {
            f(l);
            f(r);
        }
        Expr::Unary(_, inner) => f(inner),
        // Cached(slot_id, inner_expr) — slot id 在前
        Expr::Cached(_, inner) => f(inner),
        Expr::Call(_, args) => args.iter().for_each(f),
        Expr::Index(inner, _) => f(inner),
        _ => {}
    }
}

/// 对整棵树跑全部 lint 规则，返回告警清单（调用方决定如何呈现）。
pub fn lint_tree(tree: &Tree) -> Vec<String> {
    let mut out = Vec::new();
    for (id, node) in &tree.nodes {
        if let Node::Quant { branches, .. } = node {
            for b in branches {
                let where_ = format!("node '{id}' when \"{}\"", b.when_src);
                l1_check(&b.when, &where_, &mut out);
                l2_check(&b.when, &where_, &mut out);
                if let Some(Strength::Expr(se)) = &b.strength {
                    // L1 仅针对 bool 条件；strength 是标量表达式，恒假陷阱不适用
                    l2_check(se, &format!("node '{id}' strength"), &mut out);
                }
            }
        }
    }
    for (id, leaf) in &tree.leaves {
        if let Weight::Expr(we) = &leaf.weight {
            l2_check(we, &format!("leaf '{id}' weight"), &mut out);
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::loader::load_tree_str;

    fn yaml_one_branch(when: &str) -> String {
        format!(
            r#"
meta: {{ name: t, forward_window: 2, stances: [long, flat] }}
root: gate
nodes:
  gate:
    type: quant
    branches:
      - {{ when: "{when}", goto: leaf_l, label: b }}
    default: {{ goto: leaf_f, label: d }}
leaves:
  leaf_l: {{ stance: long }}
  leaf_f: {{ stance: flat }}
"#
        )
    }

    #[test]
    fn l1_flags_constant_false_breakout() {
        // 经典 A1 陷阱：窗口含当前 bar，恒假
        let t = load_tree_str(&yaml_one_branch("close > highest(high, 20)")).unwrap();
        let w = lint_tree(&t);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("gate") && w[0].contains("恒假"), "{w:?}");
        // 镜像：lowest + < ；以及反转操作数次序
        assert_eq!(
            lint_tree(
                &load_tree_str(&yaml_one_branch("close < lowest(low, 20)")).unwrap()
            )
            .len(),
            1
        );
        assert_eq!(
            lint_tree(
                &load_tree_str(&yaml_one_branch("highest(high, 20) < close")).unwrap()
            )
            .len(),
            1
        );
        // 逐位上下文同样恒假：每位 j 的滚动窗都含当前元素 → count 恒 0 空转
        assert_eq!(
            lint_tree(&load_tree_str(&yaml_one_branch("count(close > highest(high, 3), 5) >= 1")).unwrap()).len(),
            1
        );
    }

    #[test]
    fn l1_silent_on_ref_shifted_window() {
        // ref 移窗后是合法 Turtle 突破——不得误报
        let t = load_tree_str(&yaml_one_branch("close > highest(ref(high, 1), 20)")).unwrap();
        assert!(lint_tree(&t).is_empty());
    }

    #[test]
    fn l2_flags_length_one_condition() {
        // count 条件两侧均标量形 → 布尔序列长 1 → n>1 恒弃权空转
        let t = load_tree_str(&yaml_one_branch("count(bars_held > 2, 5) >= 3")).unwrap();
        let w = lint_tree(&t);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("count") && w[0].contains("弃权"), "{w:?}");
        // barssince 同理；valuewhen 同理
        assert_eq!(
            lint_tree(
                &load_tree_str(&yaml_one_branch("barssince(pos > 0) <= 3")).unwrap()
            )
            .len(),
            1
        );
        assert_eq!(
            lint_tree(
                &load_tree_str(&yaml_one_branch("valuewhen(pos > 0, close) > 1")).unwrap()
            )
            .len(),
            1
        );
        // and 任一侧塌缩到长 1 → 整体长 1，也要报
        assert_eq!(
            lint_tree(
                &load_tree_str(&yaml_one_branch("count(close > 2 and pos > 0, 5) >= 1"))
                    .unwrap()
            )
            .len(),
            1
        );
    }

    #[test]
    fn l2_silent_on_series_conditions() {
        let ok = [
            "count(close > ema(close, 5), 5) >= 3",
            "barssince(close < ema(close, 5)) <= 3",
            "count(crossover(close, ema(close, 5)), 5) >= 1",
            "count(close >= highest(close, 2), 5) == 5", // >= 创新高事件：Ge 不在 L1 算符集，恢复递归后依然零告警
        ];
        for w in ok {
            let t = load_tree_str(&yaml_one_branch(w)).unwrap();
            assert!(lint_tree(&t).is_empty(), "false positive on: {w}");
        }
    }

    #[test]
    fn all_example_trees_lint_clean() {
        // 防误报总闸：仓库全部示例树零告警
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let examples_dir = std::path::Path::new(manifest_dir).join("examples");
        for p in std::fs::read_dir(&examples_dir).unwrap() {
            let p = p.unwrap().path();
            if p.extension().and_then(|e| e.to_str()) != Some("yaml") {
                continue;
            }
            let t = crate::tree::loader::load_tree_file(&p).unwrap();
            let w = lint_tree(&t);
            assert!(w.is_empty(), "{}: {w:?}", p.display());
        }
    }

    /// SERIES_FNS 是 eval.rs 返回形态的影子表——本测试锁死两者同步：
    /// 成员实际 eval 必须返回 Series；代表性非成员必须不返回 Series。表漂移即红。
    #[test]
    fn series_fns_shape_matches_eval_reality() {
        use crate::data::bar::{Bar, Window};
        use crate::dsl::eval::{eval, Value};
        use crate::dsl::parser::parse_str;
        use chrono::NaiveDate;
        // 内联最小 ctx（不跨模块借 eval.rs 的测试工厂）：5 根含 OHLC 的 bar（atr 需要 high/low）
        let bars: Vec<Bar> = (0..5)
            .map(|i| {
                let c = 1.0 + i as f64;
                Bar {
                    time: NaiveDate::from_ymd_opt(2024, 1, 2 + i).unwrap().and_hms_opt(10, 0, 0).unwrap(),
                    open: c - 0.1,
                    high: c + 0.2,
                    low: c - 0.3,
                    close: c,
                    volume: 100.0,
                }
            })
            .collect();
        let ctx = crate::features::context::Context {
            t: bars[4].time,
            primary: Window { bars: bars.clone() },
            context: Window { bars },
            news: None,
            aux: std::collections::BTreeMap::new(),
            sim: crate::features::context::SimState::default(),
            eval_cache: Default::default(),
        };
        // SERIES_FNS 全员逐个验证（与 lint 表逐项对应，新增成员必须两边同步）
        let series_cases = [
            "sma(close, 2)", "ema(close, 2)", "wma(close, 2)", "rsi(close, 3)",
            "atr(2)", "ref(close, 1)",
            "macd_line(close, 3, 5)", "macd_signal(close, 3, 5, 2)", "macd_hist(close, 3, 5, 2)",
            "highest(close, 2)", "lowest(close, 2)", "std(close, 2)", "slope(close, 2)",
        ];
        assert_eq!(series_cases.len(), super::SERIES_FNS.len(), "测试用例数须与 SERIES_FNS 项数一致");
        for src in series_cases {
            match eval(&parse_str(src).unwrap(), &ctx).unwrap() {
                Value::Series(_) => {}
                other => panic!("SERIES_FNS member '{src}' returned {other:?}, not Series — sync SERIES_FNS in lint.rs"),
            }
        }
        // 代表性非成员：不得返回 Series（T1 提升前的旧非成员，仍为 Scalar/Bool）
        for src in ["count(close > 1, 2)", "barssince(close > 1)", "valuewhen(close > 1, close)"] {
            let v = eval(&parse_str(src).unwrap(), &ctx).unwrap();
            assert!(!matches!(v, Value::Series(_)), "'{src}' returned Series but is NOT in SERIES_FNS");
        }
        // T2 点态提升后：pointwise_fn(Series) → Series（实参含 Series 侧则结果为 Series）
        for src in ["abs(slope(close, 2))", "sigmoid(close)", "abs(close)", "min(close, 3)", "max(close, 3)"] {
            match eval(&parse_str(src).unwrap(), &ctx).unwrap() {
                Value::Series(_) => {}
                other => panic!("'{src}' pointwise(Series) should return Series, got {other:?}"),
            }
        }
        // T2 点态提升：全标量实参仍 Scalar（守则锁）
        for src in ["abs(pos)", "min(1, pos)", "max(1, pos)", "floor(2.9)", "sign(0)", "sqrt(9)", "exp(1)", "log(1)", "pow(2, 3)"] {
            let v = eval(&parse_str(src).unwrap(), &ctx).unwrap();
            assert!(!matches!(v, Value::Series(_)), "'{src}' all-scalar args should return Scalar, got Series");
        }
        // T3 派生形态：算术含 Series 侧 → eval 实证 Series（expr_shape 推断应当匹配）
        for src in ["close * volume", "abs(close - 3)", "min(close, 3)"] {
            match eval(&parse_str(src).unwrap(), &ctx).unwrap() {
                Value::Series(_) => {}
                other => panic!("'{src}' derived expr should return Series, got {other:?}"),
            }
        }
        // T3 双标量算术仍 Scalar（守则锁的运行时实证）
        for src in ["pos * 2", "abs(pos)", "min(1, pos)", "floor(2.9)"] {
            let v = eval(&parse_str(src).unwrap(), &ctx).unwrap();
            assert!(!matches!(v, Value::Series(_)), "'{src}' all-scalar derived should return Scalar, got Series");
        }
    }

    #[test]
    fn shape_inference_tracks_lifted_arithmetic() {
        // 派生序列条件不再误报 L2：算术含 Series 侧 → Many
        let ok = [
            "count(close - open > 0, 5) >= 3",
            "count(abs(close - sma(close,3)) > 0.5, 5) >= 1",
            "barssince(close * volume > 1000) <= 5",
            "count(log(close) - log(ref(close,1)) > 0, 4) >= 2",
        ];
        for w in ok {
            let t = load_tree_str(&yaml_one_branch(w)).unwrap();
            assert!(lint_tree(&t).is_empty(), "false positive on: {w}");
        }
        // 双标量算术条件仍报 L2（守则锁的另一半）
        let bad = [
            "count(pos * 2 > 1, 5) >= 1",
            "barssince(abs(pos) > 0.5) <= 3",
            "count(min(1, pos + 0.25) > 0.5, 5) >= 1",
        ];
        for w in bad {
            let t = load_tree_str(&yaml_one_branch(w)).unwrap();
            assert_eq!(lint_tree(&t).len(), 1, "false negative on: {w}");
        }
    }
}
