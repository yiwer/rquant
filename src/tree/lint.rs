//! 加载期 lint：检出"语法合法但语义必然空转/恒假"的条件写法，eprintln 告警不阻断。
//! 规则随 DSL 形态表演进——形态推断表（expr_shape）必须与 eval.rs 实际返回形态同步。

use crate::dsl::ast::{BinaryOp, Expr};
use crate::tree::loader::{Node, Strength, Tree, Weight};

/// 表达式形态：与 eval.rs 各臂返回的 Value 形态一一对应。
#[derive(Debug, Clone, Copy, PartialEq)]
enum Shape {
    Series,
    Scalar,
}

/// 返回 Series 的内置函数（Task1 后含 highest/lowest/std/slope）。与 eval.rs 同步维护。
const SERIES_FNS: [&str; 13] = [
    "sma", "ema", "wma", "rsi", "atr", "ref",
    "macd_line", "macd_signal", "macd_hist",
    "highest", "lowest", "std", "slope",
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
        Expr::Unary(..) => Shape::Scalar,
        Expr::Binary(..) => Shape::Scalar, // 算术/比较在标量语义下归约
        Expr::Call(name, _) => {
            if SERIES_FNS.contains(&name.as_str()) {
                Shape::Series
            } else {
                Shape::Scalar // count/barssince/valuewhen/abs/min/max/sigmoid 等
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
            BinaryOp::Gt | BinaryOp::Ge => {
                (is_bare_price_ident(l).is_some() && bare_window_call(r, "highest"))
                    || (bare_window_call(l, "lowest") && is_bare_price_ident(r).is_some())
            }
            BinaryOp::Lt | BinaryOp::Le => {
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
    // count/barssince/valuewhen 的条件参数是逐位语境，L1 不适用——不递归进入条件臂。
    // 其余子节点（右侧参数、算术子树等）照常递归。
    match e {
        Expr::Call(name, args)
            if matches!(name.as_str(), "count" | "barssince" | "valuewhen") && !args.is_empty() =>
        {
            // 跳过 args[0]（条件臂），仅递归其余参数
            for a in &args[1..] {
                l1_check(a, where_, out);
            }
        }
        _ => walk_children(e, &mut |c| l1_check(c, where_, out)),
    }
}

/// L2：count/barssince/valuewhen 的条件长度类为 One → 必然弃权空转。
fn l2_check(e: &Expr, where_: &str, out: &mut Vec<String>) {
    if let Expr::Call(name, args) = e
        && matches!(name.as_str(), "count" | "barssince" | "valuewhen")
        && !args.is_empty()
        && cond_len_class(&args[0]) == LenClass::One
    {
        out.push(format!(
            "{where_}: {name}(...) 条件两侧均为标量形——逐位布尔序列长度 1，\
             将恒弃权空转；至少一侧需要序列（close/ema(...)/ref(...) 等）"
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
            "count(close >= highest(close, 2), 5) == 5", // Task1 后 highest 是序列
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
}
