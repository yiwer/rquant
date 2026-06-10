# rquant：自动模糊 DSL（strength: "auto(scale)"）— 设计文档

- **日期**：2026-06-10
- **状态**：设计已确认（三特性批次之③；①②已合并）
- **关联**：软量化谓词 `strength`（标量表达式）已落地。本设计加 `auto` 形态：对分支 `when` AST 做模糊求值，免去手写 sigmoid 公式。

---

## 1. 目标与非目标

### 目标
1. `strength: "auto"`（scale=0.02）或 `"auto(0.05)"`：软模式下该支强度 = `when` 的模糊真值。
2. `dsl/eval.rs` 加 `eval_fuzzy(expr, ctx, scale) -> Result<f64>`：
   - 比较 `>/>=`：`sigmoid(margin/denom)`，`margin = lhs−rhs`，`denom = scale·max(|lhs|,|rhs|)`；`denom ≤ 1e-12` → 0.5（双方≈0，无信息）。`</<=` 镜像（margin 取负）。
   - `==/!=` 保持硬（按布尔 1.0/0.0）。
   - `and→min、or→max、not→1−x`（Gödel）。非布尔节点 → `Error::Eval`。
3. 编译表示：`tree/loader.rs` 加 `pub enum Strength { Expr(Expr), Auto(f64) }`，`Branch.strength: Option<Strength>`；加载期解析（`"auto"`/`"auto(<f64>)"` 前缀匹配，scale ≤0 或坏格式 → 加载错；其余走 `parse_str` 现状）。
4. `quant_branch_dist`：`Auto(scale)` → `eval_fuzzy(&b.when, ctx, scale)`，NaN→0/clamp 同现状。**硬门控仍是 when；硬模式完全不变。**

### 非目标（YAGNI）
- 树级/全局 fuzzy 开关；Eq/Ne 的模糊化；其它 t-norm（积/Łukasiewicz）；自动 scale 推断。

## 2. 锁定决策

| # | 维度 | 选定 |
|---|---|---|
| 1 | 触发 | 每分支 `strength: "auto"` / `"auto(scale)"`，默认 scale=0.02 |
| 2 | 比较软化 | 相对尺度 `denom = scale·max(|lhs|,|rhs|)`；denom≈0 → 0.5 |
| 3 | 组合 | Gödel：and=min、or=max、not=1−x；Eq/Ne 硬 |
| 4 | 表示 | `Strength::Expr(Expr) | Auto(f64)`（加载期定型）|

## 3. 涟漪（编译耦合）
`Branch.strength` 类型变 → `loader.rs` 编译循环、`eval/quant.rs::quant_branch_dist` 的 strength 匹配、`quant.rs` 测试助手 `br_s`（`Some(parse_str(..))` → `Some(Strength::Expr(parse_str(..)))`）。`engine/soft.rs` 测试经 YAML 构树，不直接受影响。

## 4. 诚实边界（README 注明）
auto 适合**量纲相近的双边比较**（如 `close > sma(close,20)`）；对 `x > 0` 型比较，相对尺度会令任何非零 margin 饱和趋硬——这类请写显式 `strength` 公式。

## 5. 测试
- `eval_fuzzy`（dsl）：相等 → 0.5；above → >0.5、below → <0.5（单调）；`and`=min（构造 0.5 与 ≈1 两支）；`or`=max；`not`=1−x；`==` 硬 1/0；非布尔（如 `close`）→ Err。
- loader：`"auto"` → Auto(0.02)；`"auto(0.05)"` → Auto(0.05)；`"auto(x)"`/`"auto(-1)"` → 加载错；普通表达式仍 Expr。
- `quant_branch_dist`：Auto 支 when 为真且 above → 权重 ∈(0.5,1)、残余→default、Σ=1。
- `engine/soft`：YAML 树 `strength: "auto"` → 叶子分裂（两叶权重 ∈(0,1)、Σ=1）。
- 既有全部测试不变（硬模式零改动；显式 strength 路径不变）。

## 6. 里程碑
- **T1**（纯增量）`dsl/eval.rs` `eval_fuzzy` + 单测。
- **T2**（耦合）`loader.rs` `Strength` 枚举 + 解析 + `quant_branch_dist` Auto 臂 + `br_s` 涟漪 + loader/quant 测试。
- **T3** `engine/soft` YAML auto 测试 + README 边界说明。
