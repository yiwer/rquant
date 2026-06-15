# 选股器迭代 #1：优质驱动 + 动量倾斜（combine v2）· 设计文档

- 日期：2026-06-15
- 状态：设计已与用户逐节确认，待审阅 → writing-plans
- 范围：选股器 Phase-1 的**方法学迭代**。复用已建机制（screen/backtest/CLI/种子树），仅改**合并口径 combine + 配置**，用已建回测在深 20（2018-2026）重验。不碰广度/日频/桌面（仍 Phase 2，gated）。

## 1. 背景与目标

选股器 Phase-1 验证证伪了种子集成（spec `2026-06-15-screen-quality-speculative-design.md` + 报告 `2026-06-15-screen-phase1-validation.md`）：**「优质 × 投机」强 AND 交集近乎不相交 → avg_members 0.38（top=10 几乎全程空仓）→ 总收益 −41.9% vs 等权基准 +300%**。根因是把"投机价值"当**硬要求**（必须命中形态才入选），而稳健与投机形态很少同时出现。信号级只有**动量延续弱正**（+0.27% 前瞻），突破/超跌净负。

**迭代 #1 目标**：把"投机价值"从硬"且"降为**加分项 + 标注**——**优质驱动选股（始终持优质 top-N，根治空仓）+ 动量倾斜（已证正信号加成排名）**。诚实验证此改动是否（a）根治空仓、（b）动量倾斜是否带来超 λ=0（纯优质）的增量。

## 2. 已确认决策（brainstorming）

| # | 决策 | 选择 |
|---|---|---|
| Q1 选股语义 | AND 不相交时的规则 | **优质驱动 + 投机倾斜**：始终持优质 top-N，投机做加分；投机价值从硬"且"降为加分项 + 标签 |
| Q2 形态汰留 | 哪些形态参与倾斜 | **仅动量延续倾斜**（已证正）；突破/超跌仍计算 + 标注，但不参与选股 |
| blend 形式 | 优质与动量如何合 | **乘式 `quality × (1 + λ·tilt)`**（优质为基底必要、动量按比例加成；非加式） |
| 起始参数 | λ / 权重 | λ=1.0、tilt_setups=[动量延续]、选中等权（倾斜已在排名体现，YAGNI 不做权重倾斜） |

## 3. 合并口径 combine v2（`src/screen/combine.rs`）

`combine` 演进为优质驱动 + 倾斜模型（AND 模型已证伪，替换；既有 combine 测试改为新语义）：

- **优质分** `q = mean_finite(quality)`（不变）
- **形态投票**（不变）：每形态 `setup_vote` → 命中则进 `tags[]` + `setup_strength[tag]`
- **倾斜量** `tilt = max( setup_strength[s] for s in tilt_setups if 命中 )`，无则 0（仅 `tilt_setups`，即动量延续）
- **投机分** `speculative_score = max(所有 setup_strength)`（仅信息，不进 combined，不变）
- **合格门** `eligible = q >= q_floor`（**唯一门——去掉"tags 非空"的 AND 要求**）
- **综合分** `combined = if eligible { q * (1.0 + lambda * tilt) } else { 0.0 }`
  - 乘式 → `q=0` 永不入选（优质仍是地基/必要）；`tilt=0`（纯优质）→ `combined=q`（仍可选，根治空仓）；`tilt>0` → 按 λ 比例加成排名
- **选股** 不变：`combined=0` 的不合格股走 `select_top`（滤 score>0）自然出局 → 弱市无优质股则持现金（防御性，保留扛熊特性）；强市优质多 → 满仓
- **标注** 不变：`tags[]` 仍含所有命中形态（动量/突破/超跌）；`CombineOutput` 结构不变

`MergeParams` 加 `lambda: f64` + `tilt_setups: Vec<String>`。

## 4. 配置（`src/screen/config.rs` MergeConfig）

加两字段：`lambda`（默认 1.0）+ `tilt_setups: Vec<String>`（默认 `["动量延续"]`）。校验扩展：`lambda >= 0`；`tilt_setups` 非空且**每个必须是 `setup_trees` 的键**（否则倾斜形态永不命中=静默无倾斜，配置错误左移）。其余字段（theta_fire/vote_frac/q_floor/top/quality_layers）不变。`examples/screen/screen_v1.yaml` 加 `merge.lambda: 1.0` + `merge.tilt_setups: [动量延续]`。**种子树零改动**（仍算全部；动量树输出即倾斜量）。

## 5. 验证（已建 `screen --backtest`，深 20，2018-2026，诚实隔离倾斜增量）

1. **三档对比**：`λ=0`（纯优质驱动）vs `λ=1`（优质+动量倾斜）vs **等权基准**。
   - λ>0 若不超 λ=0 → 动量倾斜无增量（诚实负结论，倾斜可设 0 退化纯优质）。
2. **空仓根治验证**：`avg_members` 是否从 0.38 升回合理水平（强市接近 top、弱市防御性降低）。
3. **跨 regime 切片**：牛市捕获是否改善、2022 熊是否仍防御（picks vs 基准）。
4. **按标签归因**：动量 picks 前瞻是否仍正（验证倾斜信号未失效）。
5. **诚实纪律（§5.3 照旧）**：不调参美化。**预期边界**：这些优质大盘本就在等权基准内，纯优质择时大概率仍跑不赢买入持有——如实记录"优质择时 vs 买入持有"的真实差距，区分"根治空仓"（机制成功）与"跑赢基准"（可能仍不达）。

## 6. 改动文件

| 文件 | 改动 |
|---|---|
| `src/screen/combine.rs` | combine 改优质驱动+倾斜（乘式）；`MergeParams` 加 lambda/tilt_setups；测试改新语义（合格门=仅优质、tilt=仅 tilt_setups、纯优质 combined=q、tilt 加成） |
| `src/screen/config.rs` | MergeConfig 加 lambda(默认1.0)/tilt_setups(默认[动量延续]) + 校验（lambda≥0、tilt_setups⊆setup_trees 键）+ 测试 |
| `src/screen/mod.rs`（run_screen）| 构造 MergeParams 传新字段；其余不变 |
| `src/screen/backtest.rs`（run_screen_backtest）| 构造 MergeParams 传新字段；其余不变 |
| `examples/screen/screen_v1.yaml` | merge 加 lambda/tilt_setups |
| 验证报告 `docs/superpowers/2026-06-15-screen-tilt-validation.md` | 新建：三档对比 + 诚实裁决 |
| 闸 | `cargo test --workspace` + `cargo clippy --workspace --all-targets -D warnings` |

## 7. 诚实边界（非目标）

- 仍 Phase-1（方法学验证于深 20）；**不**做广度/日频/桌面（Phase 2，以验证通过为闸）。
- 仅 combine + 配置变；机制/CLI/种子树/portfolio 复用不变。
- **可能仍证伪**：若 λ>0 不超 λ=0，或整体仍跑输基准——如实记录。根治空仓 ≠ 跑赢基准；二者分别裁决。
- 乘式 blend + λ=1 + 等权是起始选择；λ/tilt_setups 经验证可调（不预先多档堆砌，YAGNI）。
- OHLCV-only、横截面语义（树内自归一 + 编排器 select_top 真横截面）不变。
