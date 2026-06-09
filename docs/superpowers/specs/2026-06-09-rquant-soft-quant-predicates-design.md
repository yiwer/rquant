# rquant：软量化谓词（多路）— 设计文档

- **日期**：2026-06-09
- **状态**：设计已确认（写 spec + 计划）
- **关联**：软遍历已合并 master（HEAD `c2e59a6`）。本设计让量化节点也真正参与软遍历（此前量化 c=1 仍硬，软效果只在 LLM 节点）。

---

## 1. 背景

软遍历（spec §16）目前：量化节点 `eval_quant` 命中支 confidence=1.0 → 软模型 `(chosen:c, default:1-c)` 全给该支，**仍硬**。本设计给量化分支加可选 `strength`（标量 DSL 表达式），软模式下量化节点产出**多路分支分布**（按 `when` 为真 + strength 的"首真泄漏"），并把 `traverse_soft` 的边模型升级为多路。硬模式与现有软-LLM 行为不变。

## 2. 目标与非目标

### 目标
1. `BranchSpec` 加可选 `strength: Option<String>`（标量 DSL，加载期编译）。
2. 软量化评估 `quant_branch_dist` 产出分支分布（首真泄漏 + clamp01 strength）。
3. `traverse_soft` 边模型升级为多路 `Vec<(goto, weight)>`；LLM 仍是 2 元 c/(1-c)。
4. 加 `sigmoid(x)` DSL 函数（authoring 助手）。
5. 硬模式（`eval_quant`/`traverse`）与现有软-LLM 行为零改动；无 strength 的树软模式量化退化为硬首真。

### 非目标（YAGNI）
- 自动模糊 DSL（比较/布尔自动软化）：尺度由作者经 `strength` 掌控，不自动推断。
- strength 影响硬模式：硬模式恒忽略 strength。
- strength 影响"走哪支"：`when`（布尔）仍硬门控选支；strength 只调强度。
- 概率校准：strength/置信仍是"伪概率"，解读谨慎（同软遍历）。

## 3. 锁定决策

| # | 维度 | 选定 |
|---|---|---|
| 1 | 分级来源 | 每分支可选 `strength` 标量 DSL 表达式 |
| 2 | 残余分配 | **多路**：首真泄漏 `w_i = remaining·s_i`，default 得最终 remaining |
| 3 | 无 strength 真支 | `s = 1.0`（→ 软模式退化硬首真，渐进采用）|
| 4 | `when` 角色 | 仍硬门控（只有 when-true 支参与），strength 只调强度 |
| 5 | squash/clamp | 加 `sigmoid(x)` DSL；任何 strength 结果代码兜底 `clamp[0,1]` |
| 6 | 边模型 | `traverse_soft` 升级多路 `Vec<(goto,weight)>`；LLM 表示为 2 元；硬零改动 |

## 4. 架构

### 4.1 schema / loader
- `tree/schema.rs`：`BranchSpec` 加 `#[serde(default)] pub strength: Option<String>`。
- `tree/loader.rs`：runtime `Branch` 加 `pub strength: Option<Expr>`；加载时若 `Some` 则 `parse_str` 编译（失败 → 加载错误，同 `when`）。

### 4.2 软量化评估（`eval/quant.rs`）
```rust
/// 软模式量化分支分布：首真泄漏 + clamp01(strength)。Σ weights ≡ 1。
pub fn quant_branch_dist(branches: &[Branch], default: &Target, ctx: &Context) -> Result<Vec<(String, f64)>>
```
算法：
```
remaining = 1.0; out = []
for b in branches:                       // 按声明顺序
    if eval_bool(&b.when, ctx)? {
        let s = match &b.strength {
            Some(e) => clamp01(eval_scalar(e, ctx)?),   // DSL 求值 → 归约标量 → clamp[0,1]
            None => 1.0,
        };
        let w = remaining * s;
        if w > 0.0 { out.push((b.goto.clone(), w)); }
        remaining *= 1.0 - s;
        if remaining <= 1e-12 { break; }
    }
out.push((default.goto.clone(), remaining));
// 合并同 goto（相加）、滤 w>0
```
- `eval_scalar`：复用 DSL 求值 + 归约（series 取最新、bool→1/0）。
- clamp01：`x.clamp(0.0, 1.0)`（NaN → 0.0，避免污染）。
- 硬 `eval_quant` 不动（首真、confidence 1.0/0.5，hard 路径用）。

### 4.3 traverse_soft 多路化（`engine/soft.rs`）
- 边：`HashMap<String, Vec<(String, f64)>>`。
- 阶段一：每节点求分布 —— `Node::Quant` → `quant_branch_dist(...)`；`Node::Llm` → eval_llm 得 Decision → `vec![(chosen, c)]` 再 push `(default, 1-c)`（若 chosen==default 合并）。push 所有 weight>0 的子节点（非叶）。
- 阶段二：`leaf_dist(id) = Σ_{(g,w) in edges[id], w>0} w · leaf_dist(g)`，叶子返回 `{id:1.0}`，记忆化。
- `retain(p>0)` + `debug_assert(sum≈1)` 保留。
- 守恒：quant Σw=1（伸缩相消 + remaining）；llm c+(1-c)=1。

### 4.4 sigmoid（DSL）
`eval_call` 加 `"sigmoid" => need 1; Value::Scalar(1/(1+e^-x))`（标量）。例：
```yaml
branches:
  - when: "close > sma(close,20)"
    strength: "sigmoid((close - sma(close,20)) / (0.02 * sma(close,20)))"
    goto: leaf_long
```

## 5. 错误处理
- strength 编译失败 → 加载错误（同 when）。
- strength 求值出错 → 冒泡 `Result`（`quant_branch_dist` 返回 `Result`）。
- NaN strength → clamp01 归 0（该支不取质量，泄漏给后续/ default）。
- 守恒：Σw=1 恒成立（数学保证 + debug_assert）。

## 6. 硬模式不变性
- `eval/quant.rs::eval_quant` 与 `engine/traversal.rs::traverse` 完全不改。
- `BacktestConfig`/Report/CLI 不变。
- 无 `strength` 字段的旧树：硬模式照旧；软模式量化退化为硬首真（第一个 when-true 支得全部质量）——与本次之前的软行为一致。

## 7. 测试
- `quant_branch_dist`（构造 Branch + ctx）：
  - 单真支无 strength → `[(g,1.0)]`。
  - 单真支 strength 0.7 → `[(g,0.7),(default,0.3)]`。
  - 两真支 s=0.6/0.5 → `[(a,0.6),(b,0.2),(default,0.2)]`（0.6；0.4·0.5=0.2；0.4·0.5=0.2）。
  - 无真支 → `[(default,1.0)]`。
- `sigmoid`：`sigmoid(0)=0.5`、`sigmoid(大正)≈1`（DSL 求值）。
- `traverse_soft` 多路：strength 量化树 → 叶子分布跨多叶、和=1；**无 strength 树 → 退化单叶（与硬同叶）**；LLM 节点仍 2 元（既有软测试不变）。
- 既有全部硬/软测试不变（硬零改动；LLM 软二元仍对）。
- e2e：含 strength 的量化树软模式跑通，engaged 合理。

## 8. 风险
1. **作者负担 / 尺度**：strength 的尺度由作者定（本设计的核心取舍）——尺度选错则软化无意义；README 给 `sigmoid(margin/scale)` 范式。
2. **伪概率**：strength 非校准概率，叶子分布解读谨慎（同软遍历）。
3. **traverse_soft 重构**：边模型改多路，须保证 LLM/硬路径逐字等价——测试覆盖（退化=硬、LLM 二元仍对、守恒）。
4. **多路放大 LLM 调用**：软模式本就评估所有可达节点；多路量化可能让更多分支子树带质量 → 更多可达 LLM 节点（有缓存）。

## 9. 里程碑
- **T1** `schema`/`loader`：`BranchSpec.strength` + runtime `Branch.strength` 编译 + 测试。
- **T2** `eval/quant.rs::quant_branch_dist`（首真泄漏 + clamp01）+ `dsl` `sigmoid` + 测试。
- **T3** `engine/soft.rs` 多路化（边模型 + leaf_dist；quant 用 quant_branch_dist、llm 2 元）+ 测试（退化=硬、LLM 仍对、守恒）。
- **T4** example(strength 树) + README 一节 + e2e。
