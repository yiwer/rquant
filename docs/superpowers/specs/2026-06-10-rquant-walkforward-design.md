# rquant：Walk-forward 验证（固定树滚动分折稳定性）— 设计文档

- **日期**：2026-06-10
- **状态**：设计已确认（写 spec + 计划）
- **关联**：master `29854c1`。树为用户固定提供、无参数优化器、逐 bar 决策无状态 ⇒ 滚动分折 = **一次回测 + 按时间分桶**，无需重跑。

---

## 1. 目标与非目标

### 目标
1. `backtest --folds K`（默认 0=关，K≥2 生效）：把对齐 `primary[warmup..]` 的决策点按**索引等分为 K 个连续折**，逐折算 `SignalStat` + 同口径 buy&hold + 时间范围，汇总 `positive_folds` 与 `worst_mean_net`。
2. 硬/软都支持：硬折统计 = active（stance≠Flat）的 net；软折统计 = engaged>0 的 expected_net。
3. `Report`/`SoftReport` 加 `walk_forward: Option<WalkForward>`（`skip_serializing_if + default`，旧 JSON 兼容）；摘要打印逐折行；HTML 复用 `bar_chart` 画各折 mean_net + 汇总行。
4. README 诚实标注：**固定树的时间稳定性分析**，非含参数寻优的完整 WFO。

### 非目标（YAGNI）
- 参数寻优/树模板/样本内选模（完整 WFO）；按日历（月/周）分折；soft position 口径的独立折线；anchored/expanding 窗口。

## 2. 锁定决策

| # | 维度 | 选定 |
|---|---|---|
| 1 | 形态 | 固定树滚动分折（一次回测分桶）|
| 2 | 分折 | 决策索引等分 K 个连续折：fold j = `[j·n/k, (j+1)·n/k)`；**空索引段折省略**（n<k 时 folds.len()<k）|
| 3 | 折内口径 | 硬=active net；软=engaged expected_net |
| 4 | 兼容 | `Option<WalkForward>` + `#[serde(skip_serializing_if="Option::is_none", default)]` |

## 3. 架构

### 3.1 纯函数（`src/backtest/walkforward.rs`，新）
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FoldMetrics { pub from: NaiveDateTime, pub to: NaiveDateTime, pub stat: SignalStat, pub buy_and_hold: f64 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalkForward { pub folds: Vec<FoldMetrics>, pub positive_folds: usize, pub worst_mean_net: f64 }

/// nets_per_point[i]：第 i 个决策点的参与净收益（未参与/未计分=None）；primary_slice 与之一一对齐。
pub fn walk_forward(nets_per_point: &[Option<f64>], primary_slice: &[Bar], k: usize) -> WalkForward
```
- 每折：`stat = signal_stat(折内 Some 值)`；`buy_and_hold = slice[hi-1].close/slice[lo].open − 1`；`from/to = slice[lo].time / slice[hi-1].time`。
- 汇总：`positive_folds` = `stat.count>0 && mean_net>0` 的折数；`worst_mean_net` = `count>0` 折的最小 mean（无 → 0.0）。
- 需 `signal_stat` 可见（已 `pub(crate)`）。

### 3.2 接线
- `BacktestConfig` 加 `pub folds: usize`；CLI `--folds`（`default_value_t = 0`）。**涟漪**：`tests/e2e.rs` 全部 `BacktestConfig{}` 字面量（grep 找全）补 `folds: 0`。
- `runner::run`：`nets = results.iter().map(|(tr, fr)| match fr { Some(f) if tr.stance != Stance::Flat => Some(f.net), _ => None }).collect()`；`folds≥2` → `Some(walk_forward(&nets, &primary[start..], cfg.folds))`。
- `soft::run_soft`：`nets = results.iter().map(|(_, s)| s.filter(|x| x.engaged > 0.0).map(|x| x.expected_net)).collect()` 同上。
- `Report`/`SoftReport` 加字段（**涟漪**：runner/run_soft 构造 + report 两个测试 + viz `sample_report`/soft viz 测试字面量补 `walk_forward: None`）。
- `print_summary`/`print_soft_summary`：`Some` 时逐折 `wf fold i/N [from→to]: n/mean/hit | bh` + `wf summary: positive P/N, worst mean`。
- `render_html`/`render_soft_html`：`Some` 时 `bar_chart`（label=`f1..fN`，value=mean_net；count=0 折 value 0）+ headline 行 `wf positive folds P/N`。签名不变（从 report 结构体读）。

## 4. 测试
- 纯函数：9 点 3 折已知值（各折 stat/bh/from/to/汇总）；含 None；全 None 折不计入 positive/worst；n<k 空折省略。
- 接线（lib 测试或 e2e）：`folds=3` → Report 含 3 折、`positive_folds` 正确；`folds=0` → `walk_forward=None` 且序列化 JSON **不含**该键；旧 JSON（无该键）反序列化成功（default）。
- e2e：上升趋势 + `--folds 3` → 软模式 3 折全正（`positive_folds==3`）。
- 既有全部测试在字面量补齐后语义不变。

## 5. 风险
1. 折内样本少 → 统计噪声大；逐折 n 显示在输出里，由用户判断。
2. 等分索引折可能跨节假日缺口；可接受（时间范围已展示）。
3. 重叠前瞻窗口跨折边界（折尾决策的收益窗伸进下折）；标注即可，不裁剪（与全局重叠警告同口径）。

## 6. 里程碑
- **T1** `walkforward.rs` 纯函数 + 单测。
- **T2** 接线（Config/cli/runner/run_soft/Report/SoftReport + serde 默认 + print + 全部字面量涟漪）+ 接线测试。
- **T3** HTML 折条形图 + e2e `--folds 3` + README（诚实标注）。
