# Eval 门槛裁决机制 · 设计文档（Phase-1）

- 日期：2026-06-14
- 状态：设计已与用户逐节确认，待审阅 → writing-plans
- 范围：把"严格 WFO 五门槛"从文档方法学固化为代码，自动从 optimize 输出算出策略级机器裁决。仅 Phase-1。
- 实现修正（落地后回填）：新模块实际命名 **`src/verdict/`**（非本文下文所写的 `src/eval/`）——因既有 `src/eval/` 已是节点求值模块（quant/llm），为避撞名改用 `verdict`。CLI 子命令对外名仍是 `eval`。下文 `src/eval/{mod,gates}.rs` 字样一律以 `src/verdict/{mod,gates}.rs` 为准。

## 1. 背景与动机

当前策略评估是 **"原始计算 + 人肉判读"** 回路：`optimize` 输出丰富的原始数（IS/OS、退化比、漂移 n_unique、top-5），但**零自动门槛判定**——五门槛 WFO（设计 §3.1）与 F-1 因子标准全在文档里、不在代码里。人工读 JSON、手工套门槛、手写报告。

这套流程**易错、不可复现、不可扩展**，并且**已经出过错**：刚结束的纯量化树弧线里，树4 的 OS 正折数被实现者写成 10/30、sonnet 规格审查也确认 10/30，**两道审查都没抓住**，最后由 opus 终审从原始 JSON 重新数才发现是 9/30（30%）。一个把门槛写成代码、直接从 fold JSON 算 `certified` 的 eval 模块，本可让这个错误根本不可能发生。

在"全面回测 + 据结果持续优化"的目标下，人肉判读会随策略数、网格规模、运行频率被放大。本设计为该回路提供机器化、可复现、可审计的门槛裁决基础设施。

## 2. 目标与范围

**Phase-1 目标**：实现 WFO 五门槛的**策略级自动裁决**——给定一个策略跨其 universe 的 N 个标的 fold JSON，产出一份机器可读的 `Verdict`（certified + 逐门槛 pass/fail + 证据 + failed_gates）。

**显式非目标（Phase-1 不做，诚实声明）**：
- factor F-1 因子预检标准（|RankIC|>0.03 ∧ |ICIR|>0.3 ∧ corr<0.7）的自动化 → 暂缓（Phase-1b）。
- 批量多策略 runner、结果库/排行、跨策略横向对比 → Phase-2/3。
- "无 edge vs regime 依赖"的叙事判读 → 仍归人；eval 只出机械裁决，不臆测 regime。
- 桌面回测中心展示裁决 → gates lib 设计为可复用，但 UI 接线暂缓。
- 门槛阈值的 CLI/配置文件可调 → 暂硬编码（见 §7）。

## 3. 已确认决策（brainstorming 逐条敲定）

| # | 决策 | 选择 |
|---|---|---|
| Q1 | 门槛覆盖范围 | **WFO 五门槛全覆盖（含跨标的聚合①⑤）**，factor F-1 暂缓 |
| Q2 | 裁决逻辑落脚 | **独立 `rquant eval` 子命令 + 纯函数库**（lib 供 CLI/桌面复用）；给 OptimizeReport 补字段 |
| Q3 | 阈值配置 | **硬编码具名常量**（`GateThresholds::default()` 编码文档方法学）；门槛①用比例（≥60%）非绝对数 |
| Q4 | 门槛④自动化程度 | **自动延伸网格**（闭环判内点） |
| 架构 | 自动延伸逻辑的家 | **方案 A：延伸在 optimize，裁决在 eval** |

## 4. 架构概览

三个隔离组件，沿用项目"纯函数 + 薄壳"惯例：

```
optimize (--auto-extend N)  ──►  OptimizeReport JSON (新增 axes/primary)
                                          │  ×N 标的
                                          ▼
                          rquant eval ──► eval::gates::certify()  ──►  Verdict (print + JSON, exit 0/1)
                                          (纯函数库, 可被桌面复用)
```

- **optimize**：找到最优并**验证其为内点**（边界逃逸延伸），把内点证据写进输出。
- **gates lib**（`src/verdict/gates.rs`）：纯函数，吃 N 个 OptimizeReport 出 Verdict。全单测。
- **eval CLI**（`Cmd::Eval` 薄臂）：读 JSON → 调 `certify` → 打印 + 落 JSON + 退出码。

## 5. 策略级裁决模型（核心）

五门槛实际作用在**策略层**（跨该策略整个 universe），不是单 JSON 层。故 eval 的自然单位是：

> **一个策略 = N 个标的的 fold JSON → 一份策略级五门槛裁决**，每道门槛按规则聚合各标的证据。

- 输入：`rquant eval --reports wfo_ma_*.json`（glob 或显式 N 路径）。
- 输出：`Verdict`——`certified: bool` + 逐门槛 `GateOutcome{gate,status,value,threshold,note}` + `failed_gates[]`；打印摘要表 + `--out verdict.json`；**认证退 0、未认证退 1**（CI/pre-commit 门）。
- 诚实边界：eval 给**机械裁决**（认证/哪门槛挂 + 证据数字）。"无 edge vs regime 依赖"叙事仍是人的活。

## 6. optimize 改动：自动延伸 + schema

### 6.1 `--auto-extend N` 旗标（opt-in，默认关）

默认关闭 → **现有 optimize 行为字节级冻结**（语义冻结友好，有行为锁测试保障，见 §9）。开启后围绕 `full_sample_best` 最优点做边界逃逸：

```
1. 正常网格寻优 → 全样本最优 P*。
2. 对每条轴 A：
   若 P*[A] 落在 A 当前值域的 min 或 max 边界：
     a. 沿越界方向按 A 的原步长追加一个候选值，扩展该轴。
     b. 在扩展后的网格上重跑寻优。
     c. 若新最优仍贴新边界 且 IS 目标较前改善 → 回到 a（最多 N 步）。
     d. 终止：最优变为内点（不贴边）/ IS 不再改善 / 已达 N 步。
3. 每轴记录 interior：内点收敛或 IS 收敛=true；达 N 步仍贴边=false（边界假象）。
```

方向：best==min 向下延伸、best==max 向上延伸。步长复用该轴原步长。仅贴边轴延伸。

### 6.2 OptimizeReport 新增字段（均 serde default，兼容旧 JSON）

```rust
pub struct AxisOutcome {
    pub name: String,
    pub final_values: Vec<f64>,   // 延伸后该轴实际候选值（升序）
    pub best_value: Option<f64>,  // 全样本最优在该轴的取值
    pub interior: bool,           // best 是否落在 final_values 内部（或延伸已收敛）
    pub extended_steps: usize,    // 实际追加的延伸步数（0=无需延伸）
}

// OptimizeReport 追加：
pub axes: Vec<AxisOutcome>,       // 默认空 Vec（未开 --auto-extend 时为空）
pub primary: String,             // 标的标识（主数据路径/symbol），默认 ""——eval 用作 symbol 标签
```

### 6.3 成本与简化（诚实注记）

- 延伸需重跑寻优（每步重扩网格），跨标的 ×10 放大算力；N 默认小（4），且仅贴边轴延伸。
- Phase-1 延伸**围绕全样本最优**做（非逐折逐参数 IS 曲线分析），是对手工逐折延伸的合理简化。文档明示此简化。
- 门槛④降级：JSON 的 `axes` 为空（未开 --auto-extend）→ eval 把④判为保守"非内点"，note 标"重跑 --auto-extend 以判定"。

## 7. gates 纯函数库（`src/verdict/gates.rs`）

### 7.1 阈值与类型

```rust
pub struct GateThresholds {            // ::default() = 文档方法学编码
    pub os_positive_symbol_frac: f64,   // ① 0.6
    pub min_degradation: f64,           // ② 0.5
    pub degradation_symbol_frac: f64,   // ② 0.6
    pub drift_stable_unique_frac: f64,  // ③ 0.5  (参数 n_unique ≤ ⌈frac×OS折数⌉ 为稳)
    pub drift_stable_symbol_frac: f64,  // ③ 0.6  (稳标的占比下限)
    pub drift_consensus_frac: f64,      // ③ 0.6  (跨标的众数共识下限)
    pub interior_symbol_frac: f64,      // ④ 0.6
    pub max_single_symbol_os_share: f64,// ⑤ 0.5
}

pub enum GateStatus { Pass, Fail, Indeterminate }  // Indeterminate ≠ Pass（保守）

pub struct GateOutcome {
    pub gate: String,        // "T1_os_breadth" / "T2_degradation" / ...
    pub status: GateStatus,
    pub value: f64,          // 计算出的指标
    pub threshold: f64,
    pub note: String,        // 人读解释
}

pub struct Verdict {
    pub strategy: String,
    pub n_symbols: usize,
    pub certified: bool,             // 五门全 Pass
    pub gates: Vec<GateOutcome>,     // 5 条
    pub failed_gates: Vec<String>,
}

pub fn certify(reports: &[(String, OptimizeReport)], th: &GateThresholds) -> Verdict;
```

### 7.2 五门槛聚合公式（输入＝N 个 (symbol, OptimizeReport)）

| 门 | 名 | 公式 | Pass 条件 |
|---|---|---|---|
| ① | T1_os_breadth | 有 ≥1 OS 正折（os_objective>0）的标的数 / N | value ≥ `os_positive_symbol_frac` |
| ② | T2_degradation | 每标的取其非空 per-fold `degradation` 中位数，>`min_degradation` 为"健康"；健康标的数 / 可判定标的数 | value ≥ `degradation_symbol_frac`；全无有效 degradation 折的标的=Indeterminate 排除分母；若可判定标的 < N/2 → 门 Indeterminate |
| ③ | T3_param_drift | 标的内：每参数 `n_unique` ≤ ⌈`drift_stable_unique_frac`×OS折数⌉ 为稳，标的所有参数稳=该标的稳。跨标的：每参数 full_sample_best 取值的众数一致占比（full_sample_best 为 None 的标的不计入共识分母）。value = min(稳标的占比, 各参数最小共识占比)；threshold = `drift_consensus_frac` | 稳标的占比 ≥ `drift_stable_symbol_frac` **且** 每参数共识 ≥ `drift_consensus_frac`（即 value ≥ threshold，二者阈值同为 0.6） |
| ④ | T4_interior | 所有轴 interior=true 的标的数 / N（axes 为空的标的保守计非内点，note 标重跑提示） | value ≥ `interior_symbol_frac` |
| ⑤ | T5_not_single | 各标的"正 OS 折之和"占全体正 OS 总和的份额，取最大份额 | 最大份额 ≤ `max_single_symbol_os_share` **且** 贡献（正 OS）标的数 ≥ 2 |

- `certified = 五门槛 status 全为 Pass`。Indeterminate 不算 Pass → 保守判未认证。
- 每门 `note` 写清算了什么、为何挂（如③标注是稳定性挂还是共识挂；④标注哪些标的缺 axes）。

## 8. eval CLI（`Cmd::Eval` 薄臂）

```
rquant eval --reports <glob|path...> [--name <strategy>] [--out <verdict.json>]
```

- 读每个 JSON → `(symbol, OptimizeReport)`，symbol 标签 = `report.primary` 非空时取之，否则回退取文件名 stem（兼容未带 primary 字段的旧 JSON）。`--name`（策略名）缺省时取文件名公共前缀或首个 primary。
- 调 `gates::certify(reports, &GateThresholds::default())`。
- stdout 打印门槛表：`gate | status | value | threshold | note`，末行 `CERTIFIED ✅` / `NOT CERTIFIED ❌  failed: [T2,T4]`。
- `--out` 写 `Verdict` JSON。
- **退出码**：certified → 0；否则 → 1（供 CI/pre-commit）。
- 错误处理沿用项目惯例：JSON 缺失/解析失败/版本不符左移为清晰错误；`--reports` 展开为空 → 报错。

## 9. 测试策略（TDD 黄金）

- **逐门槛单测**：合成 `OptimizeReport`（内存构造）→ 断言每门 `GateOutcome` 的 status/value，覆盖 Pass/Fail/Indeterminate 三态边界。
- **树4 回归锁（核心）**：喂入树4 真实 10 标的 OS 值（sh600030 +2/sh600519 0正…）→ 断言 OS 正折数=**9**、广度=**7/10**、①Pass、②Fail、④边界/未判定、`certified=false`。**直接钉死上一弧线手工数错（10/30）的 bug**。
- **auto-extend 单测**（optimize）：构造最优落边界的合成数据 → 断言触发延伸、`AxisOutcome.interior`/`extended_steps` 正确；IS 单调不收敛 → interior=false。
- **optimize 行为冻结锁**：`--auto-extend` 关时，既有 optimize 黄金测试输出与改造前字节级一致（axes 为空、其余字段不变）。
- **eval CLI e2e**：临时目录放 N 个合成 JSON → 跑 `eval` → 断言退出码 + verdict JSON 字段。

## 10. 改动文件

| 文件 | 改动 |
|---|---|
| `src/verdict/mod.rs`、`src/verdict/gates.rs` | 新模块：GateThresholds/GateStatus/GateOutcome/Verdict + certify() + 五门槛纯函数 |
| `src/optimize/mod.rs` | auto-extend 算法；OptimizeReport 加 `axes: Vec<AxisOutcome>` + `primary: String`；AxisOutcome 类型 |
| `src/cli/mod.rs` | `Cmd::Eval` 薄臂（参数解析 + 调 certify + 打印 + 退出码） |
| `docs/cli-reference.md` | eval 子命令 + optimize `--auto-extend` 文档 |

## 11. 诚实边界小结

- eval 出**机械裁决**，不替代人对 regime/无-edge 的叙事判读。
- 门槛④自动延伸是**全样本最优**口径的边界逃逸，非逐折 IS 曲线分析（简化）。
- 阈值是文档方法学的编码、硬编码于 `GateThresholds::default()`，可调留 Phase-2。
- 跨标的聚合假设输入的 N 个 JSON 同属一个策略、同一 universe、同一网格定义；混用不同策略的 JSON 是使用者责任（eval 不校验语义同源，仅按 primary 标注）。
