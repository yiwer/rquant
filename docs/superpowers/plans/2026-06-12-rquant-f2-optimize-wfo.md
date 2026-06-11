# rquant F-2 参数寻优 WFO（optimize 子命令）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `rquant optimize`：网格 × 锚定扩展 WFO（IS 寻优 → OS 验证），双口径目标（打分 mean_net / sim Sharpe），输出退化率、参数漂移、全样本对照、IS top-5。

**Architecture:** 在 master(HEAD `6e3ca1a`)上新增 `src/optimize/{grid.rs, mod.rs}` + loader 参数覆盖入口。语义权威=spec §3（`docs/superpowers/specs/2026-06-12-rquant-f2-optimize-wfo-design.md`，实现者先通读）。复用：walk-forward 折切分约定、runner 的 per-leaf 计分（READ `eval_point`）、soft 的 engaged 口径、sim 的 `sim_step/finalize`、`risk_metrics`。

**Tech Stack:** Rust 2024 + 既有。

> ⚠️ git 纪律：`git add` 永远点名文件；提交前 `git status --porcelain` 检查无意外暂存。

---

## 文件结构
```
新增: src/optimize/grid.rs  # GridAxis/parse_grid_axis/expand_grid（纯函数+黄金）
新增: src/optimize/mod.rs   # ObjectiveMode/EvalData/evaluate/OptimizeConfig/run_optimize/报告类型/print
改动: src/tree/loader.rs    # load_tree_str_with_overrides（load_tree_str 改薄包装）
改动: src/lib.rs            # + pub mod optimize;
改动: src/cli/mod.rs        # Cmd::Optimize
改动: tests/e2e.rs、docs/cli-reference.md、README.md
```

---

## Task 1: grid 纯函数

**Files:**
- Create: `src/optimize/grid.rs`、`src/optimize/mod.rs`（暂只 `pub mod grid;`）；Modify: `src/lib.rs`（+ `pub mod optimize;`）

- [ ] **Step 1: RED 测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_range_axis_closed_interval() {
        let a = parse_grid_axis("ma_n=10:40:5").unwrap();
        assert_eq!(a.name, "ma_n");
        assert_eq!(a.values, vec![10.0, 15.0, 20.0, 25.0, 30.0, 35.0, 40.0]);
        // 浮点容差闭端：0.1+0.1+0.1 ≈ 0.3 必须包含
        let b = parse_grid_axis("k=0.1:0.3:0.1").unwrap();
        assert_eq!(b.values.len(), 3);
        assert!((b.values[2] - 0.3).abs() < 1e-9);
    }

    #[test]
    fn parses_list_axis() {
        let a = parse_grid_axis("thr=5,15,100").unwrap();
        assert_eq!(a.values, vec![5.0, 15.0, 100.0]);
    }

    #[test]
    fn rejects_malformed() {
        assert!(parse_grid_axis("noequals").is_err());
        assert!(parse_grid_axis("=1,2").is_err());          // 空名
        assert!(parse_grid_axis("a=").is_err());            // 空值
        assert!(parse_grid_axis("a=5:1:1").is_err());       // start>stop
        assert!(parse_grid_axis("a=1:5:0").is_err());       // step=0
        assert!(parse_grid_axis("a=1,x").is_err());         // 非数
    }

    #[test]
    fn expand_cartesian_last_axis_fastest() {
        let axes = vec![
            GridAxis { name: "a".into(), values: vec![1.0, 2.0] },
            GridAxis { name: "b".into(), values: vec![10.0, 20.0] },
        ];
        let combos = expand_grid(&axes, 10).unwrap();
        let get = |i: usize, k: &str| *combos[i].get(k).unwrap();
        assert_eq!(combos.len(), 4);
        assert_eq!((get(0, "a"), get(0, "b")), (1.0, 10.0));
        assert_eq!((get(1, "a"), get(1, "b")), (1.0, 20.0)); // b 变最快
        assert_eq!((get(2, "a"), get(2, "b")), (2.0, 10.0));
    }

    #[test]
    fn rejects_duplicates_and_cap() {
        let axes = vec![
            GridAxis { name: "a".into(), values: vec![1.0] },
            GridAxis { name: "a".into(), values: vec![2.0] },
        ];
        assert!(expand_grid(&axes, 10).is_err()); // 重名
        let big = vec![GridAxis { name: "a".into(), values: (0..600).map(|i| i as f64).collect() }];
        assert!(expand_grid(&big, 500).is_err()); // 超上限
        assert!(expand_grid(&[], 10).is_err());   // 空
    }
}
```

- [ ] **Step 2: 实现**

```rust
use crate::{Error, Result};
use std::collections::BTreeMap;

/// 一个参数轴：name + 取值列表（已展开）。
#[derive(Debug, Clone)]
pub struct GridAxis {
    pub name: String,
    pub values: Vec<f64>,
}

/// 解析 "name=start:stop:step"（闭区间，容差 1e-9）或 "name=v1,v2,…"。
pub fn parse_grid_axis(s: &str) -> Result<GridAxis> {
    let (name, rhs) = s
        .split_once('=')
        .ok_or_else(|| Error::Data(format!("grid '{s}': expected name=values")))?;
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Data(format!("grid '{s}': empty param name")));
    }
    let rhs = rhs.trim();
    if rhs.is_empty() {
        return Err(Error::Data(format!("grid '{s}': empty values")));
    }
    let num = |t: &str| -> Result<f64> {
        t.trim()
            .parse::<f64>()
            .map_err(|e| Error::Data(format!("grid '{s}': bad number '{t}': {e}")))
    };
    let values = if rhs.contains(':') {
        let parts: Vec<&str> = rhs.split(':').collect();
        if parts.len() != 3 {
            return Err(Error::Data(format!("grid '{s}': range needs start:stop:step")));
        }
        let (start, stop, step) = (num(parts[0])?, num(parts[1])?, num(parts[2])?);
        if step <= 0.0 || start > stop {
            return Err(Error::Data(format!("grid '{s}': need step>0 and start<=stop")));
        }
        let mut v = Vec::new();
        let mut x = start;
        while x <= stop + 1e-9 {
            v.push(x);
            x += step;
        }
        v
    } else {
        rhs.split(',').map(num).collect::<Result<Vec<f64>>>()?
    };
    if values.is_empty() {
        return Err(Error::Data(format!("grid '{s}': no values")));
    }
    Ok(GridAxis { name: name.to_string(), values })
}

/// 笛卡尔积（CLI 声明序、末轴变最快、确定性）；重名/空/超上限 → Error。
pub fn expand_grid(axes: &[GridAxis], max_combos: usize) -> Result<Vec<BTreeMap<String, f64>>> {
    if axes.is_empty() {
        return Err(Error::Data("optimize: at least one --grid required".into()));
    }
    for (i, a) in axes.iter().enumerate() {
        if axes[..i].iter().any(|b| b.name == a.name) {
            return Err(Error::Data(format!("optimize: duplicate --grid name '{}'", a.name)));
        }
    }
    let mut total: usize = 1;
    for a in axes {
        total = total
            .checked_mul(a.values.len())
            .ok_or_else(|| Error::Data("optimize: grid size overflow".into()))?;
    }
    if total > max_combos {
        return Err(Error::Data(format!(
            "optimize: {total} combos exceed --max-combos {max_combos}; narrow the grid"
        )));
    }
    let mut combos = Vec::with_capacity(total);
    for mut idx in 0..total {
        let mut m = BTreeMap::new();
        for a in axes.iter().rev() {
            let n = a.values.len();
            m.insert(a.name.clone(), a.values[idx % n]);
            idx /= n;
        }
        combos.push(m);
    }
    Ok(combos)
}
```

- [ ] **Step 3: GREEN + clippy + Commit（`git status --porcelain` 先查）**

```bash
git add src/optimize/grid.rs src/optimize/mod.rs src/lib.rs
git commit -m "feat(optimize): grid axis parsing and deterministic cartesian expansion" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: loader 参数覆盖入口

**Files:**
- Modify: `src/tree/loader.rs`

- [ ] **Step 1: RED 测试（tokio，行为级验证覆盖生效）**

```rust
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
        // ctx：close=10 → thr=5 走 leaf_l，thr=100 走 leaf_f
        let ctx = test_ctx_close10(); // 用本文件既有测试 ctx 构造助手；没有则按 dsl::eval 测试的 ctx_from_closes 形态新建
        let llm = crate::eval::llm::LlmEvaluator::Disabled;
        assert_eq!(crate::engine::traversal::traverse(&t_low, &ctx, &llm).await.unwrap().leaf, "leaf_l");
        assert_eq!(crate::engine::traversal::traverse(&t_hi, &ctx, &llm).await.unwrap().leaf, "leaf_f");
        // 未知键 → Err 含键名
        let mut bad = BTreeMap::new();
        bad.insert("nope".to_string(), 1.0);
        let e = load_tree_str_with_overrides(yaml, &bad).unwrap_err().to_string();
        assert!(e.contains("nope"));
    }
```

- [ ] **Step 2: 实现**

现 `load_tree_str` 主体重命名/改造为：
```rust
/// 以参数覆盖加载（override 键必须存在于树 params 块；既有全部校验保留）。
pub fn load_tree_str_with_overrides(
    yaml: &str,
    overrides: &std::collections::BTreeMap<String, f64>,
) -> Result<Tree> {
    // 1. 解析 TreeSpec（既有）
    // 2. for (k, v) in overrides:
    //      spec.params 不含 k → Err(Error::Tree(format!("override param '{k}' not found in tree params")))
    //      否则 spec.params[k] = v
    // 3. 既有 build/校验流程原封不动
}

pub fn load_tree_str(yaml: &str) -> Result<Tree> {
    load_tree_str_with_overrides(yaml, &std::collections::BTreeMap::new())
}
```
（Error 变体名按本文件实际——READ 现有错误构造。）

- [ ] **Step 3: GREEN（全量回归——load_tree_str 行为必须不变）+ clippy + Commit**

```bash
git add src/tree/loader.rs
git commit -m "feat(tree): load_tree_str_with_overrides for param-sweep loading" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: evaluate 三口径

**Files:**
- Modify: `src/optimize/mod.rs`

- [ ] **Step 1: 类型 + evaluate（READ FIRST：runner `eval_point`（per-leaf horizon/weight net 计分）、`backtest/soft.rs` 的 engaged 口径与 `score_soft` 签名、`backtest/sim.rs` 的 run_sim 主循环与 `sim_step/finalize`、`CostModel` 构造）**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectiveMode { ScoreHard, ScoreSoft, Sim }

/// 评估共享数据（一次加载，多组合复用）。
pub struct EvalData<'a> {
    pub primary: &'a [Bar],
    pub context: &'a [Bar],
    pub news: &'a [NewsRecord],
    pub aux: &'a BTreeMap<String, AuxTable>,
    pub window: usize,
    pub costs: CostModel,
}

/// 在决策索引范围上评估一棵树的目标值。无可评估点 → Ok(None)。
pub async fn evaluate(
    tree: &Tree,
    data: &EvalData<'_>,
    llm: &LlmEvaluator,
    range: std::ops::Range<usize>,
    mode: ObjectiveMode,
) -> Result<Option<f64>>
```
三口径（spec §3，顺序逐点 await）：
- **ScoreHard**：逐 i：build_context(t=primary[i].time) → traverse → 叶；stance==Flat → 跳过；`forward_return(primary, i, leaf.horizon, stance, &costs)` 取 `net × leaf.weight`（与 runner eval_point 同口径）；None → 跳过；收集 → 均值（空 → None）。
- **ScoreSoft**：逐 i：traverse_soft → `score_soft(...)`（签名按实际）→ engaged 判定与 run_soft 相同口径的点取 `expected_net` → 均值（空 → None）。
- **Sim**：`range.end` 必须 ≤ `primary.len()-1`（调用方保证）；fresh SimAccount，逐 i mirror run_sim 主循环（SimState 注入/风控覆盖/树目标/`sim_step`），末尾 `finalize`；nav 点列（含每步）→ `risk_metrics(点列, acc.max_drawdown)`：`sharpe` Some → 用之，否则 `total_return = nav−1`；nav 点 < 2 → None。

- [ ] **Step 2: 测试（tokio + tempfile 不需要——直接构造 bars 内存评估）**

- 已知值（hard）：恒涨数据 + 永真 long 树（horizon 1, weight 1）→ evaluate == 手动 `forward_return` 均值（表达式断言）。
- 范围限制：前半涨后半跌的数据 → `evaluate(0..n/2)` > 0 > `evaluate(n/2..n)`。
- 全 flat 树 → Ok(None)。
- Sim：跨多日恒涨 + 入场树 → Some(有限)。

- [ ] **Step 3: GREEN + clippy + Commit**

```bash
git add src/optimize/mod.rs
git commit -m "feat(optimize): unified objective evaluate over index range (score-hard/score-soft/sim)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: run_optimize WFO 循环 + 报告 + CLI

**Files:**
- Modify: `src/optimize/mod.rs`、`src/cli/mod.rs`

- [ ] **Step 1: 报告类型 + run_optimize**

```rust
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ComboScore { pub params: BTreeMap<String, f64>, pub objective: Option<f64> }

#[derive(Debug, Serialize, Deserialize)]
pub struct FoldResult {
    pub fold: usize,                       // OS 折号（2..=K）
    pub is_from: NaiveDateTime, pub is_to: NaiveDateTime,
    pub os_from: NaiveDateTime, pub os_to: NaiveDateTime,
    pub best_params: Option<BTreeMap<String, f64>>,
    pub is_objective: Option<f64>,
    pub os_objective: Option<f64>,
    pub degradation: Option<f64>,          // os/is，仅 is > 1e-12 时
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ParamDrift { pub name: String, pub values: Vec<Option<f64>>, pub n_unique: usize }

#[derive(Debug, Serialize, Deserialize)]
pub struct OptimizeReport {
    pub mode: String,            // "score_hard"/"score_soft"/"sim"
    pub objective_name: String,  // "active_mean_net"/"engaged_mean_expected_net"/"sharpe_or_total_return"
    pub folds: usize, pub n_combos: usize,
    pub fold_results: Vec<FoldResult>,
    pub os_mean_objective: Option<f64>,
    pub full_sample_best: Option<ComboScore>,
    pub drift: Vec<ParamDrift>,
    pub is_top5: Vec<Vec<ComboScore>>,   // 每 OS 折一组
}

pub struct OptimizeConfig { /* tree_path, primary/context/news/aux 路径, window, warmup, cost_bps, folds, sim, soft, grids: Vec<String>, max_combos, out_path */ }

pub async fn run_optimize(cfg: &OptimizeConfig, llm: &LlmEvaluator) -> Result<OptimizeReport>
```
逻辑（spec §3）：
1. 读树 YAML 文本；`parse_grid_axis` × `expand_grid`；首组合试加载（未知参数名即时报错）；树含 llm 节点 → eprintln 警告一次（READ Tree 结构判断节点类型）。
2. 加载数据（mirror runner 加载段：primary/context/news/aux）；可评估范围 `warmup..len`（sim: `warmup..len-1`）；点数 < folds×2 → Error::Data。`folds < 2` → Error::Data。
3. 折边界：范围等分 K 折（mirror `walkforward` 的索引切分约定——READ 之）。
4. 预告打印：`combos × ((K−1)×2 + 1)` 趟范围评估。
5. 每 OS 折 k（折索引 1..K，0 基）：IS = 范围起点..折 k 起点；逐组合 `load_tree_str_with_overrides` + `evaluate(IS)`（None → −∞ 排序）；best（并列取组合序先者；全 −∞ → best=None）；top5 存 ComboScore（objective 存原 Option）；best Some → `evaluate(OS)`；degradation 按 spec。
6. 全样本：全范围逐组合 → full_sample_best。
7. drift：每参数 best 序列 + 唯一值数（按 bits 比较 f64 去重）。
8. `os_mean_objective` = OS Some 值均值；写 out JSON pretty；返回。
`print_optimize_summary`：头（mode/objective/combos/folds）→ 折表（IS 区间/OS 区间/best params/IS/OS/退化）→ 漂移表 → 全样本对照行（full best vs os_mean）→ 每折 top-5 紧凑块。None → "—"。

- [ ] **Step 2: CLI**

```rust
    /// Walk-forward parameter optimization (grid x anchored-expanding IS -> OS)
    Optimize {
        #[arg(long)]
        tree: PathBuf,
        #[arg(long)]
        primary: PathBuf,
        #[arg(long)]
        context: PathBuf,
        #[arg(long)]
        news: Option<PathBuf>,
        /// Repeatable: --grid "name=start:stop:step" or "name=v1,v2,..."
        #[arg(long = "grid", value_name = "NAME=VALUES")]
        grid: Vec<String>,
        #[arg(long, default_value_t = 5)]
        folds: usize,
        #[arg(long, default_value_t = false)]
        sim: bool,
        #[arg(long, default_value_t = false)]
        soft: bool,
        #[arg(long, default_value_t = 500)]
        max_combos: usize,
        #[arg(long, default_value_t = 100)]
        warmup: usize,
        #[arg(long, default_value_t = 100)]
        window: usize,
        #[arg(long, default_value_t = 10.0)]
        cost_bps: f64,
        #[arg(long = "aux", value_name = "NAME=PATH")]
        aux: Vec<String>,
        #[arg(long, default_value = "optimize_report.json")]
        out: PathBuf,
        #[arg(long, default_value = "")]
        llm_model: String,
        #[arg(long, default_value = "")]
        llm_base_url: String,
        #[arg(long, default_value = ".rquant-cache/llm")]
        llm_cache_dir: PathBuf,
    },
```
分流：LLM 构造与 aux 解析 mirror Backtest 臂；`sim && soft` → anyhow 错误（sim 目标不分软硬，soft 仅打分口径有意义——明确拒绝组合歧义）；mode 映射：sim → Sim、soft → ScoreSoft、否则 ScoreHard；`run_optimize` → `print_optimize_summary`。

- [ ] **Step 3: 全绿 + clippy + `optimize --help` + Commit**

```bash
git add src/optimize/mod.rs src/cli/mod.rs
git commit -m "feat(cli,optimize): anchored-expanding WFO loop with drift/degradation/full-sample guards" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5: e2e 植入最优黄金 + 文档 + 真数据 smoke

**Files:**
- Modify: `tests/e2e.rs`、`docs/cli-reference.md`、`README.md`

- [ ] **Step 1: e2e `optimize_finds_planted_optimum`**

合成上升数据（10→20，≥80 bar 跨多日，tempfile CSV）+ 行内树（spec §5 黄金：`params: {thr: 5.0}`、`close > thr → long else flat`、forward_window 2）+ `--grid "thr=5,15,100"` 等价的 OptimizeConfig（folds=4，warmup 小）→ `run_optimize(&cfg, &Disabled)`：
- 每折 `best_params["thr"] == 5.0`；
- `drift[0].n_unique == 1`；
- `full_sample_best.params["thr"] == 5.0`；
- `os_mean_objective` Some 且 > 0；
- out JSON 反序列化回 OptimizeReport。

- [ ] **Step 2: 文档**

- cli-reference：optimize 全旗标表 + 输出字段表 + **防过拟合判读**（退化率 < 0.5 红旗；漂移 n_unique 接近折数=参数乱跳红旗；top-5 尖峰 vs plateau；WFO 拼接显著低于全样本最优=事后偷看的差距）+ LLM 树成本提示。
- README：optimize 一节（命令示例 + 与 `--folds`（固定树分折）的区别一句 + 研究循环更新：factor 检验 → 入树 → **optimize 校准** → backtest/sim 复检）。

- [ ] **Step 3: 真数据 smoke（手动不入库）**

`fetch sh600519 --scale 60 --adjust qfq` → `optimize --tree examples/regime_adaptive_1.yaml --grid "n_trend=10,20,30" --grid "k_trend=0.05,0.10,0.15" --folds 4 --warmup 80`（打分口径）→ 记录折表/漂移/退化率 + 一句判读；清理。

- [ ] **Step 4: 全绿 + clippy + Commit（status 先查）**

```bash
git add tests/e2e.rs docs/cli-reference.md README.md
git commit -m "test+docs: WFO planted-optimum e2e, overfit interpretation guide, real-data smoke" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## 附录 A：Spec 覆盖对照（自检）
| Spec § | 实现于 |
|---|---|
| §3 grid 语法/折切分/evaluate 三口径/寻优/退化率/全样本/漂移/top5/LLM 警告 | T1-T4 |
| §4 架构（optimize 模块/loader 覆盖入口）| T1-T2 |
| §5 测试（grid/overrides/evaluate/植入最优/e2e/smoke/文档）| T1-T5 |

## 附录 B：明确不在范围（YAGNI）
- 随机/贝叶斯搜索；rolling IS；deflated Sharpe；HTML；portfolio 寻优；sim+soft 组合（明确拒绝）。
