# 纯量化决策树组合 实现计划（4 原型 × 严格 WFO 认证）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 构造 4 棵正交风格的纯量化决策树（均值回归 / Donchian 突破 / 横截面强度 v2 / 均线多头），各按其天然层次过严格 WFO（或组合诚实降级）认证，产出一份诚实判读的对比报告。

**Architecture:** 纯研究弧，零引擎改动——全部用现成 DSL（30 函数）写 YAML + 现成 CLI（backtest --sim / optimize --folds / factor / portfolio）验证。每棵树走同一流水线：构造（lint 零警告 + 真数据跑通）→ WFO 扫参/时间切片 → 五条门槛判读 → 报告。spec：`docs/superpowers/specs/2026-06-14-pure-quant-tree-portfolio-design.md`。

**Tech Stack:** rquant DSL/CLI（Rust release 二进制 `target/release/rquant.exe`）；数据 `paper/pd_*.csv`（10 日线）。

**分支：** `pure-quant-trees`（从 master 切出）。

---

## 工程师必读上下文（零仓库背景假设）

**关键事实（已核对源码）：**
1. **二进制**：`target/release/rquant.exe`（先 `cargo build --release`）。所有 CLI 跑这个，不用 `cargo run`（慢）。
2. **tree YAML schema**（参考 `examples/regime_adaptive_1.yaml` / `strength_portfolio_1.yaml`）：顶层 `meta{name,forward_window,stances}` / `params{名:f64}` / `factors{名:"DSL"}`（文档序，可引用先定义者）/ `root` / `nodes{节点:{type:quant, branches:[{when,goto,label,可选strength}], default:{goto,label}}}` / `leaves{叶:{stance, 可选weight(数或DSL表达式), 可选horizon}}` / 可选 `risk:{stop_loss:分数, max_hold_bars:整数, 可选take_profit}`。
3. **risk 块**：`stop_loss` 是**固定分数**（如 0.12=12%），不是 DSL；ATR 倍数止损写在**树内分支**（可被 WFO 扫）、risk.stop_loss 是宽灾难兜底。
4. **DSL 语义铁律**：
   - highest/lowest 是 Series；突破比较一律 `ref(highest(high, n), 1)`（前一窗口高点，防自指/前视）；
   - max_price_since_entry / entry_price / pos / bars_held / bars_since_exit / last_trip_return 是 **sim 状态量**——`--sim`/`signal` 才有真值，**打分模式恒 NaN/0**；引用它们的分支必须裹在 `pos > 0` 守卫后（或写成阻断分支，NaN→false→落空）；
   - **冷却阻断分支**：`bars_since_exit < cool_k → flat` 放在入场逻辑前；打分模式 NaN<k→false→落空到入场逻辑（纯价格条件），不退化纯 flat（P3 纪律）；
   - 负号：phase-2 已支持一元 `-x`，但本计划用 `0 - x` 更稳。
5. **gate_pos 模式**（触发树通用骨架）：root 先分 `pos>0`（持仓→查出场）vs default（空仓→冷却→入场）。打分模式 pos≡0 永走 default，故只评入场逻辑（纯价格），sim/signal 才走持仓分支——这是正确的打分降级（评"此刻该不该入场"）。
6. **lint 总闸**：`cargo test all_example_trees_lint_clean`（lint.rs）。**先确认它是 glob examples/ 还是硬编码列表**（`grep -A8 all_example_trees_lint_clean src/tree/lint.rs`）——若硬编码，新树要加进列表；若 glob，自动覆盖。每棵新树构造后此闸必须绿。
7. **WFO 命令**（单标的）：
   ```
   target/release/rquant.exe optimize --tree <t.yaml> --primary <p.csv> --context <p.csv> \
     --grid "p1=v1,v2,v3" --grid "p2=..." --grid "p3=..." --folds 4 --sim --out tmps/wfo_<x>.json
   ```
   `--context` 传 primary 同一文件（单标的无独立 context，既定做法）。输出 JSON 含逐折 IS top-5、OS 验证、退化率、参数漂移表。
8. **10 只日线**：`paper/pd_{sh600030,sh600036,sh600276,sh600519,sh600900,sh601088,sh601318,sz000333,sz000858,sz300750}.csv`。
9. **认证五门槛**（触发树，全满足才"成熟可行"）：① ≥6/10 OS 折正；② 退化率 OS/IS>0.5；③ 参数跨折低漂移；④ 最优内点（扩格复核）；⑤ 非单标的偶然。三结局：通过/无 edge/regime 依赖。

**纪律红线**：零引擎改动（只加 examples/ YAML + docs/ 报告）；git add 点名；提交英文；中间产物落 tmps/（已 gitignore）。

**验证命令（通用）**：`cargo test all_example_trees_lint_clean`（lint 闸）；`cargo test`（全量回归，确认没碰坏引擎）。

---

### Task 1: 树 1 均值回归构造 + lint + sim 回测跑通

**Files:**
- Create: `examples/mean_reversion_1.yaml`

- [ ] **Step 1: 切分支 + release 构建 + 查 lint 闸形态**

```bash
git checkout -b pure-quant-trees
cargo build --release
grep -A10 "all_example_trees_lint_clean" src/tree/lint.rs   # 确认 glob 还是硬编码列表
```
Expected: release 二进制就绪；记录 lint 闸是否自动覆盖 examples/（若硬编码列表，后续每棵树 Step 加一行注册）。

- [ ] **Step 2: 写树文件**（完整内容，照抄）

`examples/mean_reversion_1.yaml`：

```yaml
# =====================================================================
# 均值回归 · 超跌反弹 v1 — 单标的触发树（执行层，日线）
# 理论：A股短期反转异象，与 v4 趋势跟踪正交（四棵中正交性最强）。
# 结构：gate_pos 分流 → 持仓查 ATR 止损 + 回归均线出场 / 空仓查冷却 → 创伤闸 → 超卖入场。
# 纪律：stop_n*atr 在树内(WFO 可扫)，risk.stop_loss 宽兜底；冷却阻断分支；
#   打分模式 pos≡0 只评入场逻辑(纯价格/RSI)，sim/signal 才走持仓分支。
# 窗口预算：std(20)/ema(20)/highest(60) → warmup 80 够。
# =====================================================================
meta:
  name: "均值回归 · 超跌反弹 v1"
  forward_window: 8
  stances: [long, flat]

params:
  n_ma: 20
  n_std: 20
  n_rsi: 14
  n_atr: 14
  n_dd: 60
  dd_keep: 0.80
  cool_k: 3
  k_dev: 2.0
  rsi_lo: 30.0
  stop_n: 2.0

factors:
  atr_v: "atr(n_atr)"
  ema_m: "ema(close, n_ma)"
  lower_band: "ema_m - k_dev * std(close, n_std)"
  rsi_v: "rsi(close, n_rsi)"
  hi_dd: "highest(close, n_dd)"

root: gate_pos

nodes:
  gate_pos:
    type: quant
    branches:
      - when: "pos > 0"
        goto: holding
        label: in_position
    default:
      goto: cooldown_gate
      label: flat_side

  holding:
    type: quant
    branches:
      - when: "close < entry_price - stop_n * atr_v"
        goto: exit_flat
        label: atr_stop
      - when: "close > ema_m"
        goto: exit_flat
        label: revert_to_mean
    default:
      goto: hold_long
      label: stay

  cooldown_gate:
    type: quant
    branches:
      - when: "bars_since_exit < cool_k"
        goto: flat_cooldown
        label: cooling
    default:
      goto: regime_gate
      label: cool_ok

  regime_gate:
    type: quant
    branches:
      - when: "close < hi_dd * dd_keep"
        goto: flat_damaged
        label: deep_drawdown
    default:
      goto: entry
      label: intact

  entry:
    type: quant
    branches:
      - when: "close < lower_band and rsi_v < rsi_lo"
        goto: enter_long
        label: oversold_dip
    default:
      goto: flat_wait
      label: no_signal

leaves:
  enter_long:    { stance: long, weight: 1.0, horizon: 8 }
  hold_long:     { stance: long, weight: "abs(pos)", horizon: 8 }
  exit_flat:     { stance: flat }
  flat_cooldown: { stance: flat }
  flat_damaged:  { stance: flat }
  flat_wait:     { stance: flat }

risk:
  stop_loss: 0.12
  max_hold_bars: 10
```

- [ ] **Step 3: lint 闸**

```bash
# 若 Step 1 发现是硬编码列表,先把 examples/mean_reversion_1.yaml 加进 lint.rs 的列表并重编
cargo test all_example_trees_lint_clean
```
Expected: PASS（新树零警告）。若报 L1（恒假陷阱）/L2（单长度空转）告警，停下报告——条件写法需复核（不应有，close<lower_band / rsi_v<rsi_lo 都是合法非恒假比较）。

- [ ] **Step 4: 真数据 sim 回测跑通**

```bash
mkdir -p tmps
target/release/rquant.exe backtest --tree examples/mean_reversion_1.yaml \
  --primary paper/pd_sh600519.csv --context paper/pd_sh600519.csv \
  --sim --out tmps/bt_mr_600519.json
```
Expected: 无错误退出；报告含非零 n_round_trips（超卖入场应有交易；茅台跌段尤甚）。若 0 交易，确认是参数太严还是逻辑 bug（手查一两个超卖日是否进场）。读 total_return/max_drawdown/win_rate 记入 commit message。

- [ ] **Step 5: Commit**

```bash
git status --porcelain
git add examples/mean_reversion_1.yaml
# 若改了 lint.rs 列表: git add src/tree/lint.rs
git commit -m "feat(strategy): mean-reversion oversold-bounce tree v1 (lint-clean, sim runs)"
```

---

### Task 2: 树 1 WFO 扫参 + 五门槛判读

**Files:**
- Create: `tmps/wfo_mr_*.json`（中间产物，不入库）
- Modify: `docs/superpowers/2026-06-14-pure-quant-tree-portfolio.md`（创建报告 + 树 1 节）

- [ ] **Step 1: 10 标的 WFO 扫参**

```bash
for S in sh600030 sh600036 sh600276 sh600519 sh600900 sh601088 sh601318 sz000333 sz000858 sz300750; do
  target/release/rquant.exe optimize --tree examples/mean_reversion_1.yaml \
    --primary paper/pd_$S.csv --context paper/pd_$S.csv \
    --grid "k_dev=1.5,2.0,2.5" --grid "rsi_lo=25,30,35" --grid "stop_n=1.5,2.0,2.5" \
    --folds 4 --sim --out tmps/wfo_mr_$S.json
done
```
Expected: 10 份报告生成（27 组合 × 4 折，每标的几秒）。

- [ ] **Step 2: 收集判读数据**

逐份读 `tmps/wfo_mr_$S.json`：取 OS 折目标值、退化率（OS/IS）、参数漂移表（每参 n_unique）、全样本最优 vs OS 拼接对照、IS top-5。汇总成 10 行表（标的｜OS 目标｜退化率｜k_dev/rsi_lo/stop_n 共识｜漂移）。

- [ ] **Step 3: 五门槛判读 + 内点复核**

对照认证五门槛打分。若共识 stop_n 落在网格边界（1.5 或 2.5），向外扩一格复跑该参确认内点：
```bash
# 例：若多数标的 stop_n=2.5 边界,扩到 3.0 复核
target/release/rquant.exe optimize --tree examples/mean_reversion_1.yaml \
  --primary paper/pd_<标的>.csv --context paper/pd_<标的>.csv \
  --grid "stop_n=2.0,2.5,3.0" --grid "k_dev=2.0" --grid "rsi_lo=30" \
  --folds 4 --sim --out tmps/wfo_mr_edge.json
```
判定结局：认证通过 / 无 edge / regime 依赖（注明适用域）。

- [ ] **Step 4: 写报告树 1 节**

创建 `docs/superpowers/2026-06-14-pure-quant-tree-portfolio.md`，写背景 + 方法学摘要 + **树 1 节**（10 行 WFO 表 + 五门槛逐条 + 结局判定 + 若认证通过给共识定参）。诚实：跌势样本 bh 多为负，均值回归在趋势市可能失效——如实记录。

- [ ] **Step 5: Commit**

```bash
git add docs/superpowers/2026-06-14-pure-quant-tree-portfolio.md
git commit -m "docs(strategy): mean-reversion v1 WFO results + certification verdict"
```

---

### Task 3: 树 2 Donchian 突破构造 + lint + sim 回测

**Files:**
- Create: `examples/donchian_breakout_1.yaml`

- [ ] **Step 1: 写树文件**（完整内容，照抄）

```yaml
# =====================================================================
# 动量突破 · Donchian 通道 v1 — 单标的触发树（执行层，日线）
# 理论：动量为核(mom20 F-1 实证)；Donchian N 日新高是动量经典触发,
#   与 v4 Brooks 回调入场形态不同。
# 结构：gate_pos 分流 → 持仓查 ATR 吊灯 / 空仓查冷却+S1跳赢家 → 突破入场(量能+趋势确认)。
# 纪律：突破比较 ref(highest,1) 防自指；吊灯用 max_price_since_entry 状态量(承 v3)；
#   S1 跳赢家 last_trip_return>0 是阻断分支(打分模式 NaN→落空)。
# =====================================================================
meta:
  name: "动量突破 · Donchian 通道 v1"
  forward_window: 12
  stances: [long, flat]

params:
  n_break: 20
  n_vol: 20
  n_fast: 20
  n_slow: 60
  n_atr: 14
  cool_k: 6
  vol_mult: 1.5
  chand_n: 3.0
  s1_on: 1.0

factors:
  atr_v: "atr(n_atr)"
  break_hi: "highest(ref(high, 1), n_break)"
  vol_ma: "sma(volume, n_vol)"
  ema_f: "ema(close, n_fast)"
  ema_s: "ema(close, n_slow)"

root: gate_pos

nodes:
  gate_pos:
    type: quant
    branches:
      - when: "pos > 0"
        goto: holding
        label: in_position
    default:
      goto: cooldown_gate
      label: flat_side

  holding:
    type: quant
    branches:
      - when: "close < max_price_since_entry - chand_n * atr_v"
        goto: exit_flat
        label: chandelier
    default:
      goto: hold_long
      label: stay

  cooldown_gate:
    type: quant
    branches:
      - when: "bars_since_exit < cool_k"
        goto: flat_cooldown
        label: cooling
      - when: "s1_on > 0 and last_trip_return > 0"
        goto: flat_s1_skip
        label: skip_winner
    default:
      goto: entry
      label: cool_ok

  entry:
    type: quant
    branches:
      - when: "close > break_hi and volume > vol_ma * vol_mult and ema_f > ema_s"
        goto: enter_long
        label: breakout
    default:
      goto: flat_wait
      label: no_break

leaves:
  enter_long:    { stance: long, weight: 1.0, horizon: 12 }
  hold_long:     { stance: long, weight: "abs(pos)", horizon: 12 }
  exit_flat:     { stance: flat }
  flat_cooldown: { stance: flat }
  flat_s1_skip:  { stance: flat }
  flat_wait:     { stance: flat }

risk:
  stop_loss: 0.12
  max_hold_bars: 60
```

- [ ] **Step 2: lint 闸**

```bash
# 若硬编码列表,先注册 examples/donchian_breakout_1.yaml
cargo test all_example_trees_lint_clean
```
Expected: PASS。`close > break_hi` 与 v4 的 `close > rng_hi` 同形（已知 lint-clean），不应报 L1。

- [ ] **Step 3: sim 回测跑通**

```bash
target/release/rquant.exe backtest --tree examples/donchian_breakout_1.yaml \
  --primary paper/pd_sz300750.csv --context paper/pd_sz300750.csv \
  --sim --out tmps/bt_dc_300750.json
```
Expected: 无错误；宁德(300750)有趋势段，突破树应有交易。读指标记入 commit。

- [ ] **Step 4: Commit**

```bash
git status --porcelain
git add examples/donchian_breakout_1.yaml
# 若改了 lint.rs: git add src/tree/lint.rs
git commit -m "feat(strategy): donchian breakout tree v1 (lint-clean, sim runs)"
```

---

### Task 4: 树 2 Donchian WFO 扫参 + 判读

**Files:**
- Modify: `docs/superpowers/2026-06-14-pure-quant-tree-portfolio.md`（追加树 2 节）

- [ ] **Step 1: 10 标的 WFO 扫参**

```bash
for S in sh600030 sh600036 sh600276 sh600519 sh600900 sh601088 sh601318 sz000333 sz000858 sz300750; do
  target/release/rquant.exe optimize --tree examples/donchian_breakout_1.yaml \
    --primary paper/pd_$S.csv --context paper/pd_$S.csv \
    --grid "n_break=20,40,55" --grid "vol_mult=1.2,1.5,2.0" --grid "chand_n=2.5,3.0,3.5" \
    --folds 4 --sim --out tmps/wfo_dc_$S.json
done
```

- [ ] **Step 2: 收集 + 五门槛判读 + 内点复核**

同 Task 2 Step 2-3 流程：10 行表（OS/退化率/n_break/vol_mult/chand_n 共识/漂移）→ 五门槛 → 边界则扩格复核（如 n_break=55 边界→扩 70；chand_n=3.5 边界→扩 4.0）→ 结局判定。Donchian 在趋势标的(300750/600519)预期较强、震荡标的较弱——可能是 regime 依赖结局。

- [ ] **Step 3: 写报告树 2 节 + Commit**

```bash
git add docs/superpowers/2026-06-14-pure-quant-tree-portfolio.md
git commit -m "docs(strategy): donchian breakout v1 WFO results + verdict"
```

---

### Task 5: 强度 v2 第二因子前置检验（factor 工作台）

**Files:**
- Create: `tmps/factor_v2.json` / `tmps/factor_v2.html`（中间产物）
- Modify: `docs/superpowers/2026-06-14-pure-quant-tree-portfolio.md`（追加因子检验小结）

- [ ] **Step 1: 跑因子检验**（universe = 现成 deploy/universe_10.csv，10 标的）

```bash
target/release/rquant.exe factor --universe deploy/universe_10.csv \
  --factor "mom20=close / ref(close, 20) - 1" \
  --factor "vol=atr(14) / close" \
  --factor "rev5=0 - (close / ref(close, 5) - 1)" \
  --factor "trendq=count(close > ema(close, 20), 20) / 20" \
  --sample 20 --horizon 8 --layers 5 --out tmps/factor_v2.json --html tmps/factor_v2.html
```
说明：检验用**原始 per-symbol 序列**（factor 命令本身每 bar 横截面排序算 RankIC），不用 percentrank。mom20 作锚（已知 RankIC 0.109）；vol 期望负 RankIC（高波动→低前瞻收益）；rev5 是 5 日反转；trendq 趋势持续度。

- [ ] **Step 2: 按 F-1 判据选第二因子**

读 `tmps/factor_v2.json`：每因子 mean RankIC / ICIR / t / 正占比 + 相关矩阵。
**决策规则**：选 |RankIC| 最高且**与 mom20 相关<0.7** 者。预期 vol（低波动）胜出（真正正交）。
- 若 vol 入选：树 3 第二因子 = 低波动（factor2 = `0 - vol_ratio` 的 percentrank）；
- 若 rev5/trendq 更优且相关<0.7：用之；
- 若无一过 F-1 线或全部相关≥0.7：**v2 退回纯动量**（= strength-v1 复制 + 文档说明，诚实结局，Task 6 改为构造纯动量 v2 占位或跳过）。

- [ ] **Step 3: 写因子检验小结 + Commit**

报告追加"强度 v2 第二因子检验"小结（4 因子 RankIC/ICIR/相关表 + 选定结论）。

```bash
git add docs/superpowers/2026-06-14-pure-quant-tree-portfolio.md
git commit -m "docs(strategy): strength-v2 second-factor pre-validation (F-1 screen)"
```

---

### Task 6: 树 3 横截面强度 v2 构造 + lint + 组合回测

**Files:**
- Create: `examples/strength_portfolio_2.yaml`

- [ ] **Step 1: 写树文件**（以**低波动**为第二因子；若 Task 5 选了别的，替换 `lowvol_pct` 那一行因子定义 + meta.name + 注释，结构不变）

```yaml
# =====================================================================
# 横截面强度 · 动量 × 低波动 v2 — 组合层选择树（日线 top-N）
# 理论：strength-v1 纯动量 + 第二验证因子(低波动,F-1 筛选,与 mom20 相关<0.7)正交增强。
# 结构：承 v1 三道闸(波动/创伤/趋势态)+ 分级叶；强度分 = w_mom*动量分位 + (1-w_mom)*低波动分位。
# 验证：因子前置 + 时间切片 + top×reb 敏感性(无折叠 WFO——组合工具缺口)。
# 注：第二因子若 Task 5 改选,只换 lowvol_pct 行 + blend_pct 不变。
# =====================================================================
meta:
  name: "横截面强度 · 动量×低波动 v2"
  forward_window: 8
  stances: [long, flat]

params:
  n_mom: 20
  n_rank: 60
  n_q: 20
  n_trend: 20
  n_fast: 8
  n_atr: 14
  n_dd: 60
  q_vol_lo: 0.10
  q_vol_hi: 0.99
  dd_keep: 0.85
  thr_q: 0.55
  thr_hi: 0.85
  thr_mid: 0.65
  thr_lo: 0.45
  w_mom: 0.6

factors:
  atr_v: "atr(n_atr)"
  ema_t: "ema(close, n_trend)"
  ema_f: "ema(close, n_fast)"
  vol_ratio: "atr_v / close"
  vol_pct: "percentrank(vol_ratio, n_rank)"
  mom_raw: "close / ref(close, n_mom) - 1"
  mom_pct: "percentrank(mom_raw, n_rank)"
  lowvol_pct: "percentrank(0 - vol_ratio, n_rank)"
  blend_pct: "w_mom * mom_pct + (1 - w_mom) * lowvol_pct"
  trend_q: "count(close > ema_t, n_q) / n_q"
  hi_dd: "highest(close, n_dd)"

root: gate_vol

nodes:
  gate_vol:
    type: quant
    branches:
      - when: "vol_pct < q_vol_lo"
        strength: "auto(0.05)"
        goto: flat_idle
        label: dead_vol
      - when: "vol_pct > q_vol_hi"
        strength: "auto(0.05)"
        goto: flat_chaos
        label: chaos_vol
    default:
      goto: gate_damage
      label: vol_ok

  gate_damage:
    type: quant
    branches:
      - when: "close < hi_dd * dd_keep"
        strength: "auto(0.05)"
        goto: flat_damaged
        label: deep_drawdown
    default:
      goto: trend_state
      label: intact

  trend_state:
    type: quant
    branches:
      - when: "ema_f > ema_t and trend_q >= thr_q"
        strength: "auto(0.08)"
        goto: strength_bands
        label: uptrend_state
    default:
      goto: flat_weak
      label: no_trend

  strength_bands:
    type: quant
    branches:
      - when: "blend_pct >= thr_hi"
        strength: "auto(0.08)"
        goto: leaf_s_hi
        label: band_hi
      - when: "blend_pct >= thr_mid"
        strength: "auto(0.08)"
        goto: leaf_s_mid
        label: band_mid
      - when: "blend_pct >= thr_lo"
        strength: "auto(0.08)"
        goto: leaf_s_lo
        label: band_lo
    default:
      goto: leaf_s_base
      label: band_base

leaves:
  flat_idle:    { stance: flat }
  flat_chaos:   { stance: flat }
  flat_damaged: { stance: flat }
  flat_weak:    { stance: flat }
  leaf_s_hi:   { stance: long, weight: 1.0,  horizon: 8 }
  leaf_s_mid:  { stance: long, weight: 0.75, horizon: 8 }
  leaf_s_lo:   { stance: long, weight: 0.5,  horizon: 8 }
  leaf_s_base: { stance: long, weight: 0.3,  horizon: 8 }
```

- [ ] **Step 2: lint 闸**

```bash
cargo test all_example_trees_lint_clean
```
Expected: PASS。注意 `lowvol_pct: percentrank(0 - vol_ratio, n_rank)`——`0 - vol_ratio` 是序列减（phase-2 算术提升），percentrank 滚动窗，合法。`blend_pct` 是序列加权和，合法。

- [ ] **Step 3: 组合回测跑通**（soft，日线 top3/reb5，对照 strength-v1）

```bash
target/release/rquant.exe portfolio --tree examples/strength_portfolio_2.yaml \
  --universe deploy/universe_10.csv --top 3 --rebalance 5 --soft \
  --out tmps/pf_v2_top3.json
```
Expected: 无错误；输出组合收益/超额/Sharpe/平均成员。与 strength-v1 同配对照（v1：超额 +40.5pp/Sharpe 1.33/成员 2.67）。读数记入 commit。

- [ ] **Step 4: Commit**

```bash
git status --porcelain
git add examples/strength_portfolio_2.yaml
# 若改了 lint.rs: git add src/tree/lint.rs
git commit -m "feat(strategy): cross-sectional strength v2 (momentum x low-vol, lint-clean, portfolio runs)"
```

---

### Task 7: 树 3 时间切片 + 敏感性矩阵验证

**Files:**
- Modify: `docs/superpowers/2026-06-14-pure-quant-tree-portfolio.md`（追加树 3 节）

- [ ] **Step 1: top×reb 敏感性 9 格**

```bash
for TOP in 2 3 5; do for REB in 3 5 10; do
  target/release/rquant.exe portfolio --tree examples/strength_portfolio_2.yaml \
    --universe deploy/universe_10.csv --top $TOP --rebalance $REB --soft \
    --out tmps/pf_v2_t${TOP}_r${REB}.json
done; done
```
读 9 份，建超额/Sharpe 矩阵（行 top × 列 reb）。门槛：全正、无尖峰、默认 top3/reb5 在平台中部。

- [ ] **Step 2: 时间切片期外代理**

数据无内置切片旗标——用前半段独立 CSV 验证：
```bash
# 生成前 ~599 根的子集 CSV(同 strength-v1 时间切片做法)
for S in sh600030 sh600036 sh600276 sh600519 sh600900 sh601088 sh601318 sz000333 sz000858 sz300750; do
  head -600 paper/pd_$S.csv > tmps/half_pd_$S.csv
done
# 写临时 universe 指向 half_ 文件
sed 's#paper/pd_#tmps/half_pd_#' deploy/universe_10.csv > tmps/universe_half.csv
target/release/rquant.exe portfolio --tree examples/strength_portfolio_2.yaml \
  --universe tmps/universe_half.csv --top 3 --rebalance 5 --soft --out tmps/pf_v2_half.json
```
对照全期：超额/Sharpe/成员是否一致（无前后期断裂）。

- [ ] **Step 3: 与 v1 对照判读 + 写报告树 3 节**

报告追加树 3 节：9 格敏感性矩阵 + 时间切片 + **与 strength-v1 对照**（v2 是否在超额/Sharpe/回撤上改进，还是低波动因子稀释了动量 edge）。**明写降级**：无折叠 WFO，时间切片是唯一期外代理，认证强度弱于触发树。结局：改进/持平/退化。

- [ ] **Step 4: Commit**

```bash
git add docs/superpowers/2026-06-14-pure-quant-tree-portfolio.md
git commit -m "docs(strategy): strength-v2 sensitivity + time-slice vs v1 (honest downgrade)"
```

---

### Task 8: 树 4 均线多头构造 + lint + sim 回测

**Files:**
- Create: `examples/ma_stack_1.yaml`

- [ ] **Step 1: 写树文件**（完整内容，照抄）

```yaml
# =====================================================================
# 趋势均线多头排列 v1 — 单标的触发树（执行层，日线）
# 理论：经典三均线多头排列(快>中>慢),最教科书趋势跟踪。与 v4 风格最近、正交性最弱,
#   价值在干净基准趋势树(v4 是 regime 切换 Brooks,本棵是纯排列)。
# 结构：gate_pos 分流 → 持仓查吊灯/排列瓦解 / 空仓查冷却 → 多头排列入场(回踩不破)。
# 网格只扫慢线 n_s + chand_n(optimize 笛卡尔积不能扫耦合三元组),快/中线固定 10/20。
# =====================================================================
meta:
  name: "趋势均线多头排列 v1"
  forward_window: 12
  stances: [long, flat]

params:
  n_f: 10
  n_m: 20
  n_s: 55
  n_atr: 14
  cool_k: 6
  chand_n: 3.0

factors:
  atr_v: "atr(n_atr)"
  ema_f: "ema(close, n_f)"
  ema_m: "ema(close, n_m)"
  ema_s: "ema(close, n_s)"

root: gate_pos

nodes:
  gate_pos:
    type: quant
    branches:
      - when: "pos > 0"
        goto: holding
        label: in_position
    default:
      goto: cooldown_gate
      label: flat_side

  holding:
    type: quant
    branches:
      - when: "close < max_price_since_entry - chand_n * atr_v"
        goto: exit_flat
        label: chandelier
      - when: "crossunder(ema_f, ema_m)"
        goto: exit_flat
        label: stack_break
    default:
      goto: hold_long
      label: stay

  cooldown_gate:
    type: quant
    branches:
      - when: "bars_since_exit < cool_k"
        goto: flat_cooldown
        label: cooling
    default:
      goto: entry
      label: cool_ok

  entry:
    type: quant
    branches:
      - when: "ema_f > ema_m and ema_m > ema_s and close > ema_f"
        goto: enter_long
        label: bullish_stack
    default:
      goto: flat_wait
      label: no_stack

leaves:
  enter_long:    { stance: long, weight: 1.0, horizon: 12 }
  hold_long:     { stance: long, weight: "abs(pos)", horizon: 12 }
  exit_flat:     { stance: flat }
  flat_cooldown: { stance: flat }
  flat_wait:     { stance: flat }

risk:
  stop_loss: 0.12
  max_hold_bars: 120
```

- [ ] **Step 2: lint 闸**

```bash
cargo test all_example_trees_lint_clean
```
Expected: PASS。`crossunder(ema_f, ema_m)` 在 holding 节(pos>0 守卫后)，两序列事件函数，合法。三路 `and` 排列条件合法。

- [ ] **Step 3: sim 回测跑通**

```bash
target/release/rquant.exe backtest --tree examples/ma_stack_1.yaml \
  --primary paper/pd_sh600519.csv --context paper/pd_sh600519.csv \
  --sim --out tmps/bt_ma_600519.json
```
Expected: 无错误；趋势段有持仓。读指标记入 commit。

- [ ] **Step 4: Commit**

```bash
git status --porcelain
git add examples/ma_stack_1.yaml
# 若改了 lint.rs: git add src/tree/lint.rs
git commit -m "feat(strategy): ma-stack trend tree v1 (lint-clean, sim runs)"
```

---

### Task 9: 树 4 均线多头 WFO 扫参 + 判读

**Files:**
- Modify: `docs/superpowers/2026-06-14-pure-quant-tree-portfolio.md`（追加树 4 节）

- [ ] **Step 1: 10 标的 WFO 扫参**（只扫 n_s + chand_n，2 参 12 组合）

```bash
for S in sh600030 sh600036 sh600276 sh600519 sh600900 sh601088 sh601318 sz000333 sz000858 sz300750; do
  target/release/rquant.exe optimize --tree examples/ma_stack_1.yaml \
    --primary paper/pd_$S.csv --context paper/pd_$S.csv \
    --grid "n_s=40,55,60,90" --grid "chand_n=2.5,3.0,3.5" \
    --folds 4 --sim --out tmps/wfo_ma_$S.json
done
```

- [ ] **Step 2: 收集 + 五门槛判读 + 内点复核**

同 Task 2 流程：10 行表（OS/退化率/n_s/chand_n 共识/漂移）→ 五门槛 → 边界扩格（n_s=90 边界→扩 120）→ 结局。**预期**：均线多头与 v4 重叠、且纯排列在 A股震荡市易鞭打——可能无 edge 或弱于 v4，**证伪是有效产出**，不强行调参凑数。

- [ ] **Step 3: 写报告树 4 节 + Commit**

```bash
git add docs/superpowers/2026-06-14-pure-quant-tree-portfolio.md
git commit -m "docs(strategy): ma-stack v1 WFO results + verdict"
```

---

### Task 10: 对比报告收口 + 全集 lint 闸 + 收尾

**Files:**
- Modify: `docs/superpowers/2026-06-14-pure-quant-tree-portfolio.md`（横向对比 + 诚实边界 + 结论）

- [ ] **Step 1: 横向对比节**

报告补"四棵横向对比"节：表格（树｜风格｜层次｜认证结局｜OS 证据强度｜与 v4/v1 正交性｜建议去留）。归纳哪些"成熟可行"（认证通过）、哪些证伪、哪些 regime 依赖。

- [ ] **Step 2: 诚实边界 + 结论节**

照 spec §6 写诚实边界（10 标的人工名单、组合无折叠 WFO、跌势样本、证伪是产出、无 LLM 节点）。结论：认证通过的树清单 + 是否建议接纸面盘第 4/5 账本（留用户定，给建议不替决策）。

- [ ] **Step 3: 全量闸**

```bash
cargo test all_example_trees_lint_clean   # 4 棵新树全部零警告
cargo test                                 # 全量回归(确认零引擎改动没碰坏)
cargo clippy --workspace --all-targets -- -D warnings
```
Expected: 全绿。引擎测试数应与 master 一致（本弧零引擎改动；若改了 lint.rs 列表，测试数不变只是列表多 4 项）。

- [ ] **Step 4: Commit + 收尾**

```bash
git status --porcelain
git add docs/superpowers/2026-06-14-pure-quant-tree-portfolio.md
git commit -m "docs(strategy): pure-quant tree portfolio comparative report + verdicts"
```

REQUIRED SUB-SKILL: `superpowers:finishing-a-development-branch`——全量验证 → 4 选项 → 合并 master → 删分支。合并前贴近时点 `git log origin/master..master` 与 `git log master..pure-quant-trees` 查并行提交。

---

## 计划自审记录

- **Spec 覆盖**：§3.1 触发树折叠 WFO（T2/T4/T9，五门槛 + 内点复核）；§3.2 强度 v2 诚实降级（T5 因子前置 + T7 时间切片+敏感性，明写无折叠 WFO）；§4.1-4.4 四棵树构造（T1/T3/T6/T8，完整 YAML）；§5 交付（4 YAML + 报告 + 因子小结，T10 收口）；§6 诚实边界（T10 Step2）；§7 测试纪律（每棵 lint 闸 + T10 全量闸）。
- **占位符扫描**：4 棵树 YAML 完整无占位；T6 的第二因子若 Task 5 改选，明确"只换 lowvol_pct 行 + meta.name"（结构不变，非占位）；WFO 命令/网格全具体。
- **类型/命名一致性**：参数名跨 spec/plan 一致（k_dev/rsi_lo/stop_n、n_break/vol_mult/chand_n、w_mom、n_s/chand_n）；网格命令的参数名与 YAML params 块逐一对应；文件名一致（mean_reversion_1/donchian_breakout_1/strength_portfolio_2/ma_stack_1）；报告路径统一 docs/superpowers/2026-06-14-pure-quant-tree-portfolio.md。
- **可执行性复核**：树 4 只扫 n_s+chand_n（避开耦合三元组）；s1_on 固定不入网格（树 2 网格 3 参）；factor 检验用原始序列非 percentrank；risk.stop_loss 固定分数、ATR 止损在树内可扫——全部与 spec 自审一致。
- **诚实结局预置**：T2/T4/T9 都写了"无 edge/regime 依赖"是合法结局；T5 写了"v2 退回纯动量"的诚实回退；T9 明写均线多头预期可能证伪——不强行凑认证。

