# rquant：E1+E2 — 命名因子/参数 + 叶子仓位/horizon + 时间标识符 — 设计文档

- **日期**：2026-06-10
- **状态**：设计已确认（写 spec + 计划）
- **关联**：master `3155f8e`。源自"复杂多元决策树差距分析"（G1/G3/G5/G7/G9）：多因子树可维护性、仓位大小、多 horizon、时间条件、树参数化（为完整 WFO 铺路）。

---

## 1. 目标与非目标

### 目标
1. **E1** 顶层可选块 `params:`（名→f64 常量）与 `factors:`（名→DSL 表达式，**有序**）；`when`/`strength`/后续 factor 中按名引用；**加载期 AST 内联替换**，引擎/求值器零改动。
2. **E2a** 叶子可选 `weight ∈ (0,1]`（默认 1.0）与 `horizon ≥ 1`（默认 `meta.forward_window`）；硬/软打分按叶生效。
3. **E2b** DSL 标量时间标识符 `hour`/`minute`/`dow`（dow：1=周一…7=周日）。
4. 旧树 YAML 完全兼容（全部 serde default）；w=1/horizon=全局 时打分逐字一致（既有测试零改动=验收标准）。

### 非目标（YAGNI）
- factor 求值缓存（内联展开重复求值，当前规模可接受，文档注明）；杠杆（weight>1）；按 factor 名出报告；params 扫描器（本期只做参数化表达，扫描属完整 WFO）；`date`/`bar_index` 等更多时间标识符。

## 2. 锁定决策

| # | 维度 | 选定 |
|---|---|---|
| 1 | weight 范围 | (0,1]，默认 1.0，越界加载错 |
| 2 | factors 写法 | YAML map 形态，靠 serde_yaml `Mapping` 保文档序解析 |
| 3 | 引用规则 | factor 只能引用 params 与**先定义**的 factor（天然无环）|
| 4 | 实现机制 | 加载期 AST 内联替换（params→`Expr::Number`，factor→子树深拷贝）|
| 5 | engaged 口径 | `engaged += p·w`（仓位加权参与度；w=1 退化不变）|
| 6 | position_net 的 r | 仍按全局 `meta.forward_window` 取裸收益（混合 horizon 时为近似，文档注明）|
| 7 | dow | chrono `number_from_monday`（1-7）|

## 3. 架构

### 3.1 AST 替换（`src/dsl/ast.rs`）
```rust
/// 把表达式中的 Ident(name) 按 env 替换为对应子树（深拷贝）；用于 params/factors 加载期内联。
pub fn substitute(expr: &Expr, env: &HashMap<String, Expr>) -> Expr
```
递归遍历 `Unary/Binary/Call/Index`；`Ident(name)` 命中 env → clone 替换，未命中保留。

### 3.2 schema/loader（E1）
- `TreeSpec` 加 `#[serde(default)] params: HashMap<String, f64>` 与 `#[serde(default)] factors: serde_yaml::Mapping`（保序；逐项要求 string→string，否则 `Error::Tree`）。
- loader 流程：
  1. 构建 `env: HashMap<String, Expr>`：先放 params（`Expr::Number`），再**按文档序**逐个编译 factor：`parse_str(expr)` → `substitute(&e, &env)` → 校验名后插入 env。
  2. 命名校验（params 与 factors 统一）：不得与保留标识符（`close/open/high/low/volume/hour/minute/dow` 及任何 `ctx.` 前缀形式不可能由用户定义，无需特判）、16 个函数名、`auto` 冲突；params/factors 间不得重名；违者 `Error::Tree`。
  3. 编译每个 `when`/`strength(Expr)` 后同样 `substitute`。`Strength::Auto` 的模糊求值作用于**替换后**的 when AST（自然成立）。
- 引用未定义名 → 不在 env → 留作 Ident → 求值期 "unknown identifier"。**加载期提前报错**：替换后对最终 AST 做一次"未知 Ident 检查"（收集 Ident，排除内置名）→ `Error::Tree`，把错误左移到加载期。

### 3.3 叶子 weight/horizon（E2a）
- `LeafSpec` 加 `#[serde(default)] weight: Option<f64>`、`horizon: Option<usize>`；runtime `Leaf { stance, weight: f64, horizon: usize }`；loader 校验 `0 < weight ≤ 1`、`horizon ≥ 1`，缺省 1.0 / `meta.forward_window`。
- **硬打分**（runner `eval_point`）：按 `tree.leaves[trace.leaf]` 取 horizon/weight，`forward_return(primary, i, leaf.horizon, stance, costs)` 后把 `gross/net` 各 ×weight（成本线性：`w·(gross−rate)=w·net`）。
- **软打分**（`score_soft`）：逐叶 `fr = forward_return(…, leaf.horizon, …)?`；`expected_net += p·w·fr.net`；`exposure += p·w·dir`；`engaged += p·w`（非 Flat）。`position_net` 的 `r` 仍用全局 fw（决策 6）。"任一叶越界→整点 None"按各叶自己的 horizon 判。
- 下游（metrics/walk_forward/traces/HTML）吃加权后的 net，自动继承。

### 3.4 时间标识符（E2b，`src/dsl/eval.rs`）
`Expr::Ident` 求值前特判：`"hour"`→`ctx.t.hour()`、`"minute"`→`ctx.t.minute()`、`"dow"`→`ctx.t.weekday().number_from_monday()`，均 `Value::Scalar`；否则走 `resolve_series`。fuzzy 路径经 `as_scalar` 自动可用。

## 4. 错误处理
全部新错误在**加载期**：factors 非字符串项、引用未定义名（含 when/strength 的未知 Ident 检查）、命名冲突、weight/horizon 越界、factor 表达式编译失败——`Error::Tree` 带名字定位。求值期无新路径。

## 5. 测试
- substitute：params 数值代入、factor 嵌套引用、未命中保留。
- loader：factors 保序（后者引用前者成功、反向报错）；与函数名/内置名/params 重名报错；when/strength 引用未定义名加载期报错；weight 0/1.5、horizon 0 报错；旧 YAML（无新块）照常。
- 时间：固定 `ctx.t` 断言 hour/minute/dow；`dow <= 5` 在 fuzzy 下可用。
- 打分：硬 weight 0.5 已知值（0.5×0.099=0.0495）；leaf horizon 2 覆盖全局 fw=16（3 根 bar 也可计分）；软 w=1 退化逐字一致（既有测试不动）+ w=0.5 的 expected/exposure/engaged 已知值。
- e2e：带 params/factors/weight 的树全链路。
- 文档同步：`docs/tree-yaml-schema.md`、`docs/dsl-reference.md`。

## 6. 里程碑
- **T1** 时间标识符（独立最小）+ 测试。
- **T2** `substitute` + schema/loader params/factors（保序解析、命名校验、未知 Ident 左移）+ 测试。
- **T3** Leaf weight/horizon：schema/loader 校验 + 硬/软打分接线 + 退化与已知值测试。
- **T4** 文档同步 + example 树更新 + e2e。
