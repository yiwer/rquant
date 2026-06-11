use crate::dsl::ast::{substitute, Expr};
use crate::dsl::parser::parse_str;
use crate::tree::schema::{Meta, NodeSpec, Stance, Target, TreeSpec};
use crate::{Error, Result};
use std::collections::{HashMap, HashSet};
use std::path::Path;

const RESERVED_IDENTS: [&str; 14] = [
    "close", "open", "high", "low", "volume", "hour", "minute", "dow",
    "pos", "entry_price", "bars_held", "unreal_pnl",
    "max_price_since_entry", "min_price_since_entry",
];
const RESERVED_FNS: [&str; 22] = [
    "sma", "ema", "wma", "rsi", "atr", "slope", "ref", "highest", "lowest",
    "crossover", "crossunder", "macd_line", "macd_signal", "macd_hist",
    "std", "sigmoid", "auto", "abs", "max", "min", "count", "barssince",
];

fn check_name(name: &str, env: &HashMap<String, Expr>) -> Result<()> {
    if RESERVED_IDENTS.contains(&name) || RESERVED_FNS.contains(&name) {
        return Err(Error::Tree(format!(
            "name '{name}' shadows a built-in identifier/function"
        )));
    }
    if env.contains_key(name) {
        return Err(Error::Tree(format!(
            "duplicate name '{name}' in params/factors"
        )));
    }
    Ok(())
}

/// 替换后残余 Ident 必须是内置标识符或 ctx. 前缀（把"未定义名"左移到加载错）。
fn check_no_unknown_idents(expr: &Expr, where_: &str) -> Result<()> {
    match expr {
        Expr::Ident(name) => {
            if RESERVED_IDENTS.contains(&name.as_str()) || name.starts_with("ctx.") {
                return Ok(());
            }
            if let Some(rest) = name.strip_prefix("aux.") {
                return match rest.split_once('.') {
                    Some((t, c)) if !t.is_empty() && !c.is_empty() && !c.contains('.') => Ok(()),
                    _ => Err(Error::Tree(format!("{where_}: aux identifier must be aux.<table>.<column>, got '{name}'"))),
                };
            }
            Err(Error::Tree(format!(
                "{where_}: unknown identifier '{name}'"
            )))
        }
        Expr::Number(_) => Ok(()),
        Expr::Unary(_, e) | Expr::Index(e, _) | Expr::Cached(_, e) => check_no_unknown_idents(e, where_),
        Expr::Binary(_, l, r) => {
            check_no_unknown_idents(l, where_)?;
            check_no_unknown_idents(r, where_)
        }
        Expr::Call(_, args) => args.iter().try_for_each(|a| check_no_unknown_idents(a, where_)),
    }
}

/// 分支强度：显式标量表达式，或对 when 做模糊求值的 auto(scale)。
/// `auto` 默认 scale=0.02；`auto(s)` 允许自定义正数 scale。
#[derive(Debug, Clone)]
pub enum Strength {
    /// 用户提供的 DSL 标量表达式，结果 clamp 到 [0, 1]。
    Expr(Expr),
    /// 基于 when 表达式边距的模糊强度，scale 控制边距→概率的斜率。
    Auto(f64),
}

/// 量化节点的一条分支：条件、可选强度及路由目标。
#[derive(Debug, Clone)]
pub struct Branch {
    /// 编译后的条件表达式（运行时求值）。
    pub when: Expr,
    /// 原始 DSL 字符串，用于 trace rationale。
    pub when_src: String,
    /// 软遍历强度；`None` 等价于强度 1.0（硬跳转）。
    pub strength: Option<Strength>,
    /// 条件成立时跳转到的节点/叶子 ID。
    pub goto: String,
    /// 分支标签，写入 trace path。
    pub label: String,
}

/// 运行时节点：量化节点（DSL 分支）或 LLM 节点（语言模型调用）。
#[derive(Debug, Clone)]
pub enum Node {
    /// 按序求值 branches，首个 when=true 的分支获胜；全不中走 default。
    Quant {
        branches: Vec<Branch>,
        default: Target,
    },
    /// 调用 LLM，将返回的 label 映射到 goto；失败或 default 胜出时走 default。
    Llm {
        inputs: Vec<String>,
        prompt: String,
        labels: HashMap<String, String>,
        default: String,
        /// 缓存/求值作用域：judge 节点为 `judge:<名>`（共享判定 → 共享缓存键），内联节点 None（用节点 id）。
        scope: Option<String>,
    },
}

/// 叶子权重：常量（加载期校验 (0,1]）或 DSL 表达式（决策时求值，NaN→0、clamp [0,1]）。
#[derive(Debug, Clone)]
pub enum Weight {
    Const(f64),
    Expr(Expr),
}

/// 决策树叶子节点，持有最终 stance 及打分参数。
#[derive(Debug, Clone)]
pub struct Leaf {
    /// 对应的交易方向（须在 `meta.stances` 中声明）。
    pub stance: Stance,
    /// 仓位大小：常量 ∈ (0,1]（默认 1.0），或 DSL 表达式（决策时 `weight_at` 求值）。
    pub weight: Weight,
    /// 该叶前瞻评分窗口，默认 meta.forward_window
    pub horizon: usize,
}

impl Leaf {
    /// 解析叶子权重：常量直接返回；表达式按 ctx 求值。
    /// 求值失败/非有限值 → 0.0（弃权 = 不持仓），有限值 clamp 到 [0,1]。
    /// 注意：catch-all → 0.0 针对的是**运行时数据弃权**（NaN 预热/空仓哨兵）；
    /// 配置类错误（未知标识符、aux 格式错）已由加载期 check_no_unknown_idents 左移。
    /// 唯一例外是 aux 表未挂载（加载期无法知晓挂载情况）——该失败也会被映射为 0 仓位。
    pub fn weight_at(&self, ctx: &crate::features::context::Context) -> f64 {
        match &self.weight {
            Weight::Const(w) => *w,
            Weight::Expr(e) => match crate::dsl::eval::eval_scalar(e, ctx) {
                Ok(v) if v.is_finite() => v.clamp(0.0, 1.0),
                _ => 0.0,
            },
        }
    }
}

/// 风险管理块：止损、止盈、最大持仓时间。
#[derive(Debug, Clone)]
pub struct Risk {
    /// 止损幅度，必须 > 0（可选）。
    pub stop_loss: Option<f64>,
    /// 止盈幅度，必须 > 0（可选）。
    pub take_profit: Option<f64>,
    /// 最大持仓 bar 数，必须 >= 1（可选）。
    pub max_hold_bars: Option<usize>,
}

/// 加载并验证后的运行时决策树。
///
/// 验证保证：DAG（无环）、所有节点从 root 可达、
/// 所有 goto 目标存在、叶子 stance 在 `meta.stances` 中。
#[derive(Debug, Clone)]
pub struct Tree {
    /// 树级元信息（名称、前瞻窗口、允许的 stance 集合）。
    pub meta: Meta,
    /// 风险管理块（可选）。
    pub risk: Option<Risk>,
    /// 根节点 ID（必须为节点，不得为叶子）。
    pub root: String,
    /// 所有中间节点，键为节点 ID。
    pub nodes: HashMap<String, Node>,
    /// 所有叶子节点，键为叶子 ID。
    pub leaves: HashMap<String, Leaf>,
}

/// 从文件加载并验证决策树。等价于读取文件后调用 [`load_tree_str`]。
pub fn load_tree_file(path: &Path) -> Result<Tree> {
    let src = std::fs::read_to_string(path)?;
    load_tree_str(&src)
}

/// 以参数覆盖加载决策树（override 键必须存在于树 params 块；既有全部校验保留）。
///
/// 验证规则：root 必须是节点；所有 goto 目标已定义；从 root 可达所有节点；
/// 无环（DFS 着色）；叶子 stance 在 `meta.stances` 声明集合内。
pub fn load_tree_str_with_overrides(
    src: &str,
    overrides: &std::collections::BTreeMap<String, f64>,
) -> Result<Tree> {
    let mut spec: TreeSpec = serde_yaml::from_str(src)?;

    // Apply overrides: each override key must exist in spec.params
    for (k, v) in overrides {
        if !spec.params.contains_key(k) {
            return Err(Error::Tree(format!("override param '{k}' not found in tree params")));
        }
        spec.params.insert(k.clone(), *v);
    }

    let stances: HashSet<Stance> = spec.meta.stances.iter().copied().collect();

    // Build substitution environment: params first, then factors (document order).
    let mut env: HashMap<String, Expr> = HashMap::new();
    for (k, v) in &spec.params {
        check_name(k, &env)?;
        env.insert(k.clone(), Expr::Number(*v));
    }
    for (slot, (k, v)) in (0_u32..).zip(&spec.factors) {
        let name = k
            .as_str()
            .ok_or_else(|| Error::Tree("factor name must be a string".into()))?;
        let src_expr = v
            .as_str()
            .ok_or_else(|| Error::Tree(format!("factor '{name}' expr must be a string")))?;
        check_name(name, &env)?;
        let e = parse_str(src_expr)
            .map_err(|e| Error::Tree(format!("factor '{name}': {e}")))?;
        let e = substitute(&e, &env);
        check_no_unknown_idents(&e, &format!("factor '{name}'"))?;
        // 包缓存槽：所有引用处共享同一槽位 → 每个 Context 只真算一次（params 是字面量，不包）。
        // INVARIANT：槽位 id 必须全树唯一（本计数器是唯一分配点）——id 撞车会造成静默值串用。
        // 布尔因子同样包裹：硬 when 多处引用照常命中；fuzzy strength 路径对 Cached 透传重算
        //（fuzzy 真值依赖 scale，不消费 Value 缓存），正确性不受影响、只是该路径无缓存收益。
        let e = Expr::Cached(slot, Box::new(e));
        env.insert(name.to_string(), e);
    }

    let mut leaves = HashMap::new();
    for (id, l) in &spec.leaves {
        if !stances.contains(&l.stance) {
            return Err(Error::Tree(format!(
                "leaf '{id}' stance {:?} not in meta.stances",
                l.stance
            )));
        }
        let weight = match &l.weight {
            None => Weight::Const(1.0),
            Some(serde_yaml::Value::Number(n)) => {
                let w = n
                    .as_f64()
                    .ok_or_else(|| Error::Tree(format!("leaf '{id}' weight must be a number")))?;
                if !(w > 0.0 && w <= 1.0) {
                    return Err(Error::Tree(format!(
                        "leaf '{id}' weight must be in (0,1], got {w}"
                    )));
                }
                Weight::Const(w)
            }
            Some(serde_yaml::Value::String(s)) => {
                let e = parse_str(s).map_err(|e| Error::Tree(format!("leaf '{id}' weight: {e}")))?;
                let e = substitute(&e, &env);
                check_no_unknown_idents(&e, &format!("leaf '{id}' weight"))?;
                // 带引号的纯数字（"0.5"，或 params 替换后坍缩成字面量的 "unit"）
                // 按常量处理，套用与数值形式相同的 (0,1] 加载期校验——防止引号绕过范围检查
                if let Expr::Number(w) = e {
                    if !(w > 0.0 && w <= 1.0) {
                        return Err(Error::Tree(format!(
                            "leaf '{id}' weight must be in (0,1], got {w}"
                        )));
                    }
                    Weight::Const(w)
                } else {
                    Weight::Expr(e)
                }
            }
            Some(_) => {
                return Err(Error::Tree(format!(
                    "leaf '{id}' weight must be a number or a DSL expression string"
                )))
            }
        };
        let horizon = l.horizon.unwrap_or(spec.meta.forward_window);
        if horizon == 0 {
            return Err(Error::Tree(format!(
                "leaf '{id}' horizon must be >= 1"
            )));
        }
        leaves.insert(id.clone(), Leaf { stance: l.stance, weight, horizon });
    }

    let mut nodes = HashMap::new();
    for (id, ns) in &spec.nodes {
        match ns {
            NodeSpec::Quant { branches, default } => {
                let mut compiled = Vec::new();
                for b in branches {
                    let expr = parse_str(&b.when).map_err(|e| {
                        Error::Tree(format!("node '{id}' branch '{}': {e}", b.label))
                    })?;
                    let expr = substitute(&expr, &env);
                    let where_when =
                        format!("node '{id}' branch '{}'", b.label);
                    check_no_unknown_idents(&expr, &where_when)?;
                    let strength = match &b.strength {
                        Some(s_src) => {
                            let st = parse_strength(s_src).map_err(|e| {
                                Error::Tree(format!(
                                    "node '{id}' branch '{}' strength: {e}",
                                    b.label
                                ))
                            })?;
                            // Substitute and check Strength::Expr; Auto uses the
                            // already-substituted `when` so needs no extra work.
                            let st = match st {
                                Strength::Expr(se) => {
                                    let se = substitute(&se, &env);
                                    let where_st = format!(
                                        "node '{id}' branch '{}' strength",
                                        b.label
                                    );
                                    check_no_unknown_idents(&se, &where_st)?;
                                    Strength::Expr(se)
                                }
                                other => other,
                            };
                            Some(st)
                        }
                        None => None,
                    };
                    compiled.push(Branch {
                        when: expr,
                        when_src: b.when.clone(),
                        strength,
                        goto: b.goto.clone(),
                        label: b.label.clone(),
                    });
                }
                nodes.insert(
                    id.clone(),
                    Node::Quant {
                        branches: compiled,
                        default: default.clone(),
                    },
                );
            }
            NodeSpec::Llm { inputs, prompt, labels, judge, map, default } => {
                let node = match judge {
                    Some(jname) => {
                        if !prompt.is_empty() || !labels.is_empty() || !inputs.is_empty() {
                            return Err(Error::Tree(format!(
                                "llm node '{id}': judge form must not also set prompt/labels/inputs"
                            )));
                        }
                        let j = spec.judges.get(jname).ok_or_else(|| {
                            Error::Tree(format!("llm node '{id}': unknown judge '{jname}'"))
                        })?;
                        if j.labels.is_empty() {
                            return Err(Error::Tree(format!("judge '{jname}': labels must be non-empty")));
                        }
                        for k in map.keys() {
                            if !j.labels.contains(k) {
                                return Err(Error::Tree(format!(
                                    "llm node '{id}': map key '{k}' not in judge '{jname}' labels"
                                )));
                            }
                        }
                        // 物化 label→goto：judge 的每个 label 都有落点（未映射 → 本节点 default）。
                        // 物化后键集 = judge.labels，与共享同一 judge 的其它节点逐字节同渲染。
                        let labels: HashMap<String, String> = j
                            .labels
                            .iter()
                            .map(|l| (l.clone(), map.get(l).cloned().unwrap_or_else(|| default.clone())))
                            .collect();
                        Node::Llm {
                            inputs: j.inputs.clone(),
                            prompt: j.prompt.clone(),
                            labels,
                            default: default.clone(),
                            scope: Some(format!("judge:{jname}")),
                        }
                    }
                    None => {
                        if !map.is_empty() {
                            return Err(Error::Tree(format!("llm node '{id}': 'map' requires 'judge'")));
                        }
                        Node::Llm {
                            inputs: inputs.clone(),
                            prompt: prompt.clone(),
                            labels: labels.clone(),
                            default: default.clone(),
                            scope: None,
                        }
                    }
                };
                nodes.insert(id.clone(), node);
            }
        }
    }

    let risk = if let Some(r) = spec.risk {
        let stop_loss = r.stop_loss;
        let take_profit = r.take_profit;
        let max_hold_bars = r.max_hold_bars;

        // Validate: stop_loss and take_profit must be > 0 if present
        if let Some(sl) = stop_loss
            && sl <= 0.0
        {
            return Err(Error::Tree(format!("stop_loss must be > 0, got {sl}")));
        }
        if let Some(tp) = take_profit
            && tp <= 0.0
        {
            return Err(Error::Tree(format!("take_profit must be > 0, got {tp}")));
        }
        // Validate: max_hold_bars must be >= 1 if present
        if let Some(mh) = max_hold_bars
            && mh < 1
        {
            return Err(Error::Tree(format!("max_hold_bars must be >= 1, got {mh}")));
        }

        Some(Risk { stop_loss, take_profit, max_hold_bars })
    } else {
        None
    };

    let tree = Tree {
        meta: spec.meta.clone(),
        risk,
        root: spec.root.clone(),
        nodes,
        leaves,
    };
    validate(&tree)?;
    Ok(tree)
}

/// 从 YAML 字符串加载并验证决策树。
///
/// 验证规则：root 必须是节点；所有 goto 目标已定义；从 root 可达所有节点；
/// 无环（DFS 着色）；叶子 stance 在 `meta.stances` 声明集合内。
pub fn load_tree_str(src: &str) -> Result<Tree> {
    load_tree_str_with_overrides(src, &std::collections::BTreeMap::new())
}

/// "auto" → Auto(0.02)；"auto(<f64>)" → Auto(s)（s>0）；其余按 DSL 表达式编译。
fn parse_strength(src: &str) -> Result<Strength> {
    let s = src.trim();
    if s == "auto" {
        return Ok(Strength::Auto(0.02));
    }
    if let Some(inner) = s.strip_prefix("auto(").and_then(|r| r.strip_suffix(')')) {
        let scale: f64 = inner
            .trim()
            .parse()
            .map_err(|_| Error::Tree(format!("bad auto scale '{inner}'")))?;
        if scale <= 0.0 {
            return Err(Error::Tree(format!("auto scale must be > 0, got {scale}")));
        }
        return Ok(Strength::Auto(scale));
    }
    Ok(Strength::Expr(parse_str(s)?))
}

fn node_targets(node: &Node) -> Vec<String> {
    match node {
        Node::Quant { branches, default } => {
            let mut v: Vec<String> = branches.iter().map(|b| b.goto.clone()).collect();
            v.push(default.goto.clone());
            v
        }
        Node::Llm { labels, default, .. } => {
            let mut v: Vec<String> = labels.values().cloned().collect();
            v.push(default.clone());
            v
        }
    }
}

fn validate(tree: &Tree) -> Result<()> {
    let exists = |id: &str| tree.nodes.contains_key(id) || tree.leaves.contains_key(id);

    if !tree.nodes.contains_key(&tree.root) {
        return Err(Error::Tree(format!("root '{}' is not a node", tree.root)));
    }

    for (id, node) in &tree.nodes {
        for tgt in node_targets(node) {
            if !exists(&tgt) {
                return Err(Error::Tree(format!(
                    "node '{id}' points to unknown target '{tgt}'"
                )));
            }
        }
    }

    // reachability from root
    let mut seen = HashSet::new();
    let mut stack = vec![tree.root.clone()];
    while let Some(cur) = stack.pop() {
        if !seen.insert(cur.clone()) {
            continue;
        }
        if let Some(node) = tree.nodes.get(&cur) {
            for tgt in node_targets(node) {
                stack.push(tgt);
            }
        }
    }
    for id in tree.nodes.keys() {
        if !seen.contains(id) {
            return Err(Error::Tree(format!("node '{id}' unreachable from root")));
        }
    }

    // DAG check
    let mut color: HashMap<String, u8> = HashMap::new();
    dfs_cycle(&tree.root, tree, &mut color)?;
    Ok(())
}

fn dfs_cycle(cur: &str, tree: &Tree, color: &mut HashMap<String, u8>) -> Result<()> {
    color.insert(cur.to_string(), 1); // 1 = in stack
    if let Some(node) = tree.nodes.get(cur) {
        for tgt in node_targets(node) {
            match color.get(&tgt).copied().unwrap_or(0) {
                1 => return Err(Error::Tree(format!("cycle detected at '{tgt}'"))),
                0 => dfs_cycle(&tgt, tree, color)?,
                _ => {}
            }
        }
    }
    color.insert(cur.to_string(), 2); // 2 = done
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const JUDGE_TREE: &str = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
judges:
  news_veto:
    inputs: [news_score]
    prompt: "消息面是否一票否决？"
    labels: [veto, pass]
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "close > 1", goto: g1, label: up } ]
    default: { goto: g2, label: dn }
  g1:
    type: llm
    judge: news_veto
    map: { veto: leaf_f, pass: leaf_l }
    default: leaf_f
  g2:
    type: llm
    judge: news_veto
    map: { veto: leaf_f }
    default: leaf_l
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;

    #[test]
    fn judge_nodes_materialize_labels_and_scope() {
        let tree = load_tree_str(JUDGE_TREE).unwrap();
        let (g1, g2) = (tree.nodes.get("g1").unwrap(), tree.nodes.get("g2").unwrap());
        match (g1, g2) {
            (
                Node::Llm { labels: l1, inputs: i1, prompt: p1, scope: s1, .. },
                Node::Llm { labels: l2, prompt: p2, scope: s2, .. },
            ) => {
                // 物化：judge 的每个 label 都有落点；g2 未映射的 pass → 其 default(leaf_l)
                assert_eq!(l1["veto"], "leaf_f");
                assert_eq!(l1["pass"], "leaf_l");
                assert_eq!(l2["veto"], "leaf_f");
                assert_eq!(l2["pass"], "leaf_l");
                // 键集一致（渲染串一致的前提）+ prompt/inputs 来自 judge + scope 一致
                let mut k1: Vec<_> = l1.keys().collect();
                let mut k2: Vec<_> = l2.keys().collect();
                k1.sort(); k2.sort();
                assert_eq!(k1, k2);
                assert_eq!(p1, "消息面是否一票否决？");
                assert_eq!(p2, p1);
                assert_eq!(i1, &vec!["news_score".to_string()]);
                assert_eq!(s1.as_deref(), Some("judge:news_veto"));
                assert_eq!(s1, s2);
            }
            _ => panic!("expected llm nodes"),
        }
    }

    #[test]
    fn judge_node_with_empty_map_routes_all_to_default() {
        let src = JUDGE_TREE.replace("map: { veto: leaf_f, pass: leaf_l }\n    default: leaf_f", "default: leaf_f");
        let tree = load_tree_str(&src).unwrap();
        match tree.nodes.get("g1").unwrap() {
            Node::Llm { labels, .. } => {
                assert_eq!(labels["veto"], "leaf_f");
                assert_eq!(labels["pass"], "leaf_f");
            }
            _ => panic!("expected llm node"),
        }
    }

    #[test]
    fn judge_validation_rejects_bad_forms() {
        // 未知 judge
        assert!(load_tree_str(&JUDGE_TREE.replace("judge: news_veto", "judge: nope")).is_err());
        // judge 形式不得再带 prompt
        assert!(load_tree_str(&JUDGE_TREE.replace(
            "    judge: news_veto\n    map: { veto: leaf_f, pass: leaf_l }",
            "    judge: news_veto\n    prompt: \"x\"\n    map: { veto: leaf_f, pass: leaf_l }"
        )).is_err());
        // map 键不在 judge labels 内
        assert!(load_tree_str(&JUDGE_TREE.replace("map: { veto: leaf_f, pass: leaf_l }", "map: { nope: leaf_f }")).is_err());
        // map 不带 judge（改成内联形式 + 残留 map）
        let inline_with_map = JUDGE_TREE.replace(
            "    judge: news_veto\n    map: { veto: leaf_f, pass: leaf_l }",
            "    prompt: \"q\"\n    labels: { veto: leaf_f }\n    map: { veto: leaf_f }");
        assert!(load_tree_str(&inline_with_map).is_err());
        // judge labels 为空
        assert!(load_tree_str(&JUDGE_TREE.replace("labels: [veto, pass]", "labels: []")).is_err());
        // 内联形式（无 judge）完全不受影响
        let inline = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
root: g1
nodes:
  g1:
    type: llm
    prompt: "q"
    labels: { yes: leaf_l }
    default: leaf_f
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;
        let t = load_tree_str(inline).unwrap();
        match t.nodes.get("g1").unwrap() {
            Node::Llm { scope, .. } => assert!(scope.is_none()),
            _ => panic!(),
        }
    }

    const VALID: &str = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
root: a
nodes:
  a:
    type: quant
    branches:
      - when: "close > sma(close,3)"
        goto: leaf_l
        label: up
    default: { goto: leaf_f, label: none }
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;

    #[test]
    fn loads_valid_tree() {
        let tree = load_tree_str(VALID).unwrap();
        assert_eq!(tree.root, "a");
        assert_eq!(tree.nodes.len(), 1);
        assert_eq!(tree.leaves.len(), 2);
    }

    #[test]
    fn rejects_unknown_target() {
        let bad = VALID.replace("goto: leaf_l", "goto: nope");
        assert!(load_tree_str(&bad).is_err());
    }

    #[test]
    fn rejects_leaf_stance_not_in_meta() {
        let bad = VALID.replace("leaf_l: { stance: long }", "leaf_l: { stance: short }");
        assert!(load_tree_str(&bad).is_err());
    }

    #[test]
    fn rejects_bad_dsl_at_load() {
        let bad = VALID.replace(r#"when: "close > sma(close,3)""#, r#"when: "close >""#);
        assert!(load_tree_str(&bad).is_err());
    }

    #[test]
    fn rejects_cycle() {
        let cyc = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "close > 1", goto: b, label: x } ]
    default: { goto: leaf_f, label: none }
  b:
    type: quant
    branches: [ { when: "close > 1", goto: a, label: y } ]
    default: { goto: leaf_f, label: none }
leaves:
  leaf_f: { stance: flat }
"#;
        assert!(load_tree_str(cyc).is_err());
    }

    #[test]
    fn loads_branch_strength() {
        let src = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "close > 1", strength: "sigmoid(close - 1)", goto: leaf_l, label: up } ]
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;
        let tree = load_tree_str(src).unwrap();
        let n = tree.nodes.get("a").unwrap();
        match n {
            crate::tree::loader::Node::Quant { branches, .. } => assert!(branches[0].strength.is_some()),
            _ => panic!("expected quant"),
        }
    }

    #[test]
    fn bad_strength_expr_errors() {
        let src = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "close > 1", strength: "sigmoid(", goto: leaf_l, label: up } ]
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;
        assert!(load_tree_str(src).is_err());
    }

    // H2 — root not a node (root points at a leaf)
    #[test]
    fn rejects_root_that_is_a_leaf() {
        let bad = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
root: leaf_l
nodes:
  a:
    type: quant
    branches:
      - when: "close > 1"
        goto: leaf_l
        label: up
    default: { goto: leaf_f, label: none }
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;
        let err = load_tree_str(bad).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("root"), "expected 'root' in error message, got: {msg}");
    }

    // H3 — unreachable node
    #[test]
    fn rejects_unreachable_node() {
        let bad = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
root: a
nodes:
  a:
    type: quant
    branches:
      - when: "close > 1"
        goto: leaf_l
        label: up
    default: { goto: leaf_f, label: none }
  orphan:
    type: quant
    branches:
      - when: "close > 1"
        goto: leaf_l
        label: up
    default: { goto: leaf_f, label: none }
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;
        let err = load_tree_str(bad).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unreachable"), "expected 'unreachable' in error message, got: {msg}");
    }

    #[test]
    fn params_and_factors_inline_and_validate() {
        let ok = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
params: { th: 2.0 }
factors:
  mom: "close - th"
  momp: "mom > 0"
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "momp and mom > th", goto: leaf_l, label: up } ]
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;
        assert!(load_tree_str(ok).is_ok());

        // Forward reference: mom references momp which is defined after it → load error.
        let bad_order = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
params: { th: 2.0 }
factors:
  mom: "close - momp"
  momp: "mom > 0"
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "momp and mom > th", goto: leaf_l, label: up } ]
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;
        assert!(load_tree_str(bad_order).is_err());

        // Name clash with built-in function "sma" → load error.
        assert!(load_tree_str(&ok.replace("mom:", "sma:")).is_err());

        // Name clash with built-in shift function "ref" → load error.
        // (standalone snippet: the only defect is the factor name itself)
        let shadow_ref = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
factors:
  ref: "close - 1"
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "ref > 0", goto: leaf_l, label: up } ]
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;
        assert!(
            load_tree_str(shadow_ref).is_err(),
            "factor named 'ref' must be rejected as shadowing a built-in"
        );

        // Name clash with built-in identifier "close" → load error.
        assert!(load_tree_str(&ok.replace("th: 2.0", "close: 2.0")).is_err());

        // `when` referencing undefined name → load-time error (left-shift).
        let unknown = ok.replace("momp and mom > th", "nope > 0");
        assert!(load_tree_str(&unknown).is_err());
    }

    #[test]
    fn leaf_weight_and_horizon_validated_and_defaulted() {
        // Explicit weight=0.5 and horizon=8 are read correctly.
        let ok = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "close > 0", goto: leaf_l, label: up } ]
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long, weight: 0.5, horizon: 8 }
  leaf_f: { stance: flat }
"#;
        let tree = load_tree_str(ok).unwrap();
        let l = tree.leaves.get("leaf_l").unwrap();
        assert!(matches!(l.weight, Weight::Const(w) if (w - 0.5).abs() < 1e-12));
        assert_eq!(l.horizon, 8);
        // Defaults: weight=1.0, horizon=forward_window (3).
        let lf = tree.leaves.get("leaf_f").unwrap();
        assert!(matches!(lf.weight, Weight::Const(w) if (w - 1.0).abs() < 1e-12));
        assert_eq!(lf.horizon, 3);

        // weight=0.0 → Err
        let w0 = ok.replace("weight: 0.5", "weight: 0.0");
        assert!(load_tree_str(&w0).is_err());
        // weight=1.5 → Err
        let w15 = ok.replace("weight: 0.5", "weight: 1.5");
        assert!(load_tree_str(&w15).is_err());
        // horizon=0 → Err
        let h0 = ok.replace("horizon: 8", "horizon: 0");
        assert!(load_tree_str(&h0).is_err());
    }

    /// 构建最小 Context 供 weight_at 测试（loader tests 此前不依赖 Context）。
    fn mini_ctx() -> crate::features::context::Context {
        use crate::data::bar::{Bar, Window};
        let t = chrono::NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        let bars = vec![Bar { time: t, open: 10.0, high: 10.0, low: 10.0, close: 10.0, volume: 1.0 }];
        crate::features::context::Context {
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
    fn leaf_weight_expression_loads_and_evaluates() {
        let ok = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
params: { unit: 0.25 }
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "close > 0", goto: leaf_l, label: up } ]
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long, weight: "min(1, pos + unit)" }
  leaf_f: { stance: flat }
"#;
        let tree = load_tree_str(ok).unwrap();
        let l = tree.leaves.get("leaf_l").unwrap();
        assert!(matches!(l.weight, Weight::Expr(_)));
        // 非 sim ctx：pos=0 → 0.25
        let mut ctx = mini_ctx();
        assert!((l.weight_at(&ctx) - 0.25).abs() < 1e-12);
        // sim 注入 pos=0.5 → 0.75
        ctx.sim.pos = 0.5;
        assert!((l.weight_at(&ctx) - 0.75).abs() < 1e-12);
        // clamp：pos=0.9 → min(1, 1.15) = 1.0
        ctx.sim.pos = 0.9;
        assert!((l.weight_at(&ctx) - 1.0).abs() < 1e-12);
        // NaN → 0（弃权）：表达式引用空仓 entry_price
        let nan_w = ok.replace("min(1, pos + unit)", "entry_price / 100");
        let tree2 = load_tree_str(&nan_w).unwrap();
        assert!((tree2.leaves["leaf_l"].weight_at(&mini_ctx()) - 0.0).abs() < 1e-12);
        // 数值路径不变：Const + 旧校验
        let tree3 = load_tree_str(&ok.replace(r#""min(1, pos + unit)""#, "0.5")).unwrap();
        assert!(matches!(tree3.leaves["leaf_l"].weight, Weight::Const(w) if (w - 0.5).abs() < 1e-12));
        // 未知标识符 / 坏语法 → 加载错
        assert!(load_tree_str(&ok.replace("pos + unit", "nope + 1")).is_err());
        assert!(load_tree_str(&ok.replace("min(1, pos + unit)", "min(1,")).is_err());
        // 带引号的纯数字坍缩为 Const 并套用 (0,1] 校验——引号不能绕过范围检查
        let quoted = load_tree_str(&ok.replace("min(1, pos + unit)", "0.5")).unwrap();
        assert!(matches!(quoted.leaves["leaf_l"].weight, Weight::Const(w) if (w - 0.5).abs() < 1e-12));
        assert!(load_tree_str(&ok.replace("min(1, pos + unit)", "1.5")).is_err());
        // params 替换后坍缩成字面量的同理
        let collapsed = load_tree_str(&ok.replace("min(1, pos + unit)", "unit")).unwrap();
        assert!(matches!(collapsed.leaves["leaf_l"].weight, Weight::Const(w) if (w - 0.25).abs() < 1e-12));
    }

    #[test]
    fn loads_factor_tree_example() {
        let src = include_str!("../../examples/factor_tree.yaml");
        assert!(load_tree_str(src).is_ok(), "examples/factor_tree.yaml must load without error");
    }

    #[test]
    fn loads_regime_adaptive_example() {
        let src = include_str!("../../examples/regime_adaptive_1.yaml");
        let tree = load_tree_str(src)
            .expect("examples/regime_adaptive_1.yaml must load without error");
        assert_eq!(tree.root, "gate_pos");
    }

    #[test]
    fn loads_sim_tree_example() {
        let src = include_str!("../../examples/sim_tree.yaml");
        let tree = load_tree_str(src).expect("examples/sim_tree.yaml must load without error");
        // Verify risk block is parsed correctly
        let risk = tree.risk.as_ref().expect("sim_tree.yaml must have a risk block");
        assert!((risk.stop_loss.unwrap() - 0.05).abs() < 1e-12);
        assert_eq!(risk.max_hold_bars, Some(60));
        assert!(risk.take_profit.is_none());
        // Verify the tree uses sim identifiers (pos) in branches
        assert_eq!(tree.root, "gate");
        assert!(tree.nodes.contains_key("gate"));
        assert!(tree.leaves.contains_key("leaf_full"));
        assert!(tree.leaves.contains_key("leaf_flat"));
    }

    #[test]
    fn aux_identifier_format_validated_at_load() {
        let yaml = |when: &str| format!(r#"
meta: {{ name: t, forward_window: 3, stances: [long, flat] }}
root: a
nodes:
  a:
    type: quant
    branches: [ {{ when: "{when}", goto: leaf_l, label: up }} ]
    default: {{ goto: leaf_f, label: flat }}
leaves:
  leaf_l: {{ stance: long }}
  leaf_f: {{ stance: flat }}
"#);
        assert!(load_tree_str(&yaml("aux.idx.close > 0")).is_ok());
        assert!(load_tree_str(&yaml("aux.idx > 0")).is_err());
    }

    #[test]
    fn parses_auto_strength_variants() {
        let yaml = |s: &str| format!(r#"
meta: {{ name: t, forward_window: 3, stances: [long, flat] }}
root: a
nodes:
  a:
    type: quant
    branches: [ {{ when: "close > 1", strength: "{s}", goto: leaf_l, label: up }} ]
    default: {{ goto: leaf_f, label: flat }}
leaves:
  leaf_l: {{ stance: long }}
  leaf_f: {{ stance: flat }}
"#);
        let get = |s: &str| -> Option<Strength> {
            let tree = load_tree_str(&yaml(s)).ok()?;
            match tree.nodes.get("a")? {
                Node::Quant { branches, .. } => branches[0].strength.clone(),
                _ => None,
            }
        };
        assert!(matches!(get("auto"), Some(Strength::Auto(s)) if (s - 0.02).abs() < 1e-12));
        assert!(matches!(get("auto(0.05)"), Some(Strength::Auto(s)) if (s - 0.05).abs() < 1e-12));
        assert!(matches!(get("0.7"), Some(Strength::Expr(_))));
        assert!(load_tree_str(&yaml("auto(x)")).is_err());
        assert!(load_tree_str(&yaml("auto(-1)")).is_err());
    }

    #[test]
    fn sim_identifiers_are_reserved() {
        let yaml = |param: &str| format!(r#"
meta: {{ name: t, forward_window: 3, stances: [long, flat] }}
params: {{ {param}: 1.0 }}
root: a
nodes:
  a:
    type: quant
    branches: [ {{ when: "close > 0", goto: leaf_l, label: up }} ]
    default: {{ goto: leaf_f, label: flat }}
leaves:
  leaf_l: {{ stance: long }}
  leaf_f: {{ stance: flat }}
"#);
        assert!(load_tree_str(&yaml("pos")).is_err());
        assert!(load_tree_str(&yaml("entry_price")).is_err());
        assert!(load_tree_str(&yaml("bars_held")).is_err());
        assert!(load_tree_str(&yaml("unreal_pnl")).is_err());
        assert!(load_tree_str(&yaml("max_price_since_entry")).is_err());
        assert!(load_tree_str(&yaml("min_price_since_entry")).is_err());
    }

    #[test]
    // 注意：本测试与下个测试用 Debug 渲染匹配 "Cached(N,"——若 Expr 的 Debug 派生格式变化会误红，
    // 意图是断言「槽 0 被 ≥2 处共享、嵌套因子槽位互异」。
    fn factors_are_wrapped_in_shared_cache_slots() {
        let src = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
factors:
  atr_v: "atr(3)"
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "atr_v >= 0 and atr_v < 1000", goto: leaf_l, label: up } ]
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;
        let tree = load_tree_str(src).unwrap();
        // 同名因子的两处引用共享同一槽位 id（Cached(0, ...) 出现 ≥2 次）
        let rendered = format!("{:?}", tree.nodes.get("a").unwrap());
        assert!(
            rendered.matches("Cached(0,").count() >= 2,
            "expected shared cache slot, got: {rendered}"
        );
    }

    #[test]
    fn nested_factors_get_distinct_slots() {
        let src = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
factors:
  base: "sma(close, 3)"
  derived: "base + 1"
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "derived > 0 and base > 0", goto: leaf_l, label: up } ]
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;
        let tree = load_tree_str(src).unwrap();
        let rendered = format!("{:?}", tree.nodes.get("a").unwrap());
        // derived 包槽 1，其体内嵌 base 的槽 0；branch 里直接引用的 base 也是槽 0
        assert!(rendered.contains("Cached(1,"), "derived slot missing: {rendered}");
        assert!(rendered.matches("Cached(0,").count() >= 2, "base slot not shared: {rendered}");
    }

    #[test]
    fn math_fns_are_reserved() {
        let yaml = |factor: &str| format!(r#"
meta: {{ name: t, forward_window: 3, stances: [long, flat] }}
factors:
  {factor}: "close - 1"
root: a
nodes:
  a:
    type: quant
    branches: [ {{ when: "{factor} > 0", goto: leaf_l, label: up }} ]
    default: {{ goto: leaf_f, label: flat }}
leaves:
  leaf_l: {{ stance: long }}
  leaf_f: {{ stance: flat }}
"#);
        assert!(load_tree_str(&yaml("abs")).is_err());
        assert!(load_tree_str(&yaml("max")).is_err());
        assert!(load_tree_str(&yaml("min")).is_err());
        assert!(load_tree_str(&yaml("count")).is_err());
        assert!(load_tree_str(&yaml("barssince")).is_err());
    }

    #[test]
    fn risk_block_parsed_and_validated() {
        let yaml = |risk: &str| format!(r#"
meta: {{ name: t, forward_window: 3, stances: [long, flat] }}
{risk}
root: a
nodes:
  a:
    type: quant
    branches: [ {{ when: "close > 1", goto: leaf_l, label: up }} ]
    default: {{ goto: leaf_f, label: flat }}
leaves:
  leaf_l: {{ stance: long }}
  leaf_f: {{ stance: flat }}
"#);
        let t = load_tree_str(&yaml("risk: { stop_loss: 0.05, max_hold_bars: 60 }")).unwrap();
        let r = t.risk.as_ref().unwrap();
        assert!((r.stop_loss.unwrap() - 0.05).abs() < 1e-12);
        assert_eq!(r.max_hold_bars, Some(60));
        assert!(r.take_profit.is_none());
        assert!(load_tree_str(&yaml("")).unwrap().risk.is_none());
        assert!(load_tree_str(&yaml("risk: { stop_loss: -0.1 }")).is_err());
        assert!(load_tree_str(&yaml("risk: { max_hold_bars: 0 }")).is_err());
    }

    // Test helper: construct a minimal context with close=10.0
    fn test_ctx_close10() -> crate::features::context::Context {
        use crate::data::bar::Bar;
        use chrono::NaiveDate;
        let base = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        let bars = vec![Bar {
            time: base,
            open: 10.0,
            high: 10.0,
            low: 10.0,
            close: 10.0,
            volume: 1.0,
        }];
        crate::features::context::Context {
            t: base,
            primary: crate::data::bar::Window { bars: bars.clone() },
            context: crate::data::bar::Window { bars },
            news: None,
            aux: std::collections::BTreeMap::new(),
            sim: crate::features::context::SimState::default(),
            eval_cache: Default::default(),
        }
    }

    #[tokio::test]
    async fn overrides_change_routing_and_unknown_key_errs() {
        let yaml = r#"
meta: { name: t, forward_window: 2, stances: [long, flat] }
params: { thr: 5.0 }
root: gate
nodes:
  gate:
    type: quant
    branches: [ { when: "close > thr", goto: leaf_l, label: above } ]
    default: { goto: leaf_f, label: below }
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;
        use std::collections::BTreeMap;
        let mut hi = BTreeMap::new();
        hi.insert("thr".to_string(), 100.0);
        let t_low = load_tree_str(yaml).unwrap();                    // thr=5
        let t_hi = load_tree_str_with_overrides(yaml, &hi).unwrap(); // thr=100
        // ctx: close=10 → thr=5 走 leaf_l, thr=100 走 leaf_f
        let ctx = test_ctx_close10();
        let llm = crate::eval::llm::LlmEvaluator::Disabled;
        assert_eq!(crate::engine::traversal::traverse(&t_low, &ctx, &llm).await.unwrap().leaf, "leaf_l");
        assert_eq!(crate::engine::traversal::traverse(&t_hi, &ctx, &llm).await.unwrap().leaf, "leaf_f");
        // 未知键 → Err 含键名
        let mut bad = BTreeMap::new();
        bad.insert("nope".to_string(), 1.0);
        let e = load_tree_str_with_overrides(yaml, &bad).unwrap_err().to_string();
        assert!(e.contains("nope"));
    }
}
