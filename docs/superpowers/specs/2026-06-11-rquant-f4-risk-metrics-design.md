# rquant：F-4 — 风险指标集 — 设计文档

- **日期**：2026-06-11
- **状态**：设计已确认（写 spec + 计划）
- **关联**：master `189f6ec`。成熟度差距分析 F-4（风险组合层第一条）：报告只有 total_return/max_dd/win_rate/turnover，连 Sharpe 都没有；补齐"风险的通用语言"。

---

## 1. 目标与非目标

### 目标
1. `report/risk.rs` 纯函数：净值序列 → `RiskMetrics { ann_return, ann_vol, sharpe, sortino, calmar (各 Option), var95, cvar95 }`。
2. 挂载：`SimReport.risk` 与 `PortfolioReport.risk`（`Option<RiskMetrics>`，serde skip/default 旧 JSON 兼容）；打分模式 `SignalStat.t_stat: Option<f64>`。
3. print 摘要与 sim/portfolio HTML headline 各加行（None → "—"）。

### 非目标（YAGNI）
- 打分模式挂 Sharpe（重叠前瞻窗口非净值序列，语义误导）；基准 RiskMetrics；β/月度收益表；rf ≠ 0；参数化分位（固定 95%）。

## 2. 锁定决策
| # | 维度 | 选定 |
|---|---|---|
| 1 | 年化基准 | **时间戳推断**：`bars_per_year = n_rets / 跨度年数`；跨度 < 30 天 → 年化族 None（拒绝误导）|
| 2 | 覆盖 | sim + portfolio 全套；打分模式仅 t_stat |
| 3 | 除零 | vol≈0 / dd≈0 / 无负收益 → 相应比率 None，绝不给假数字 |

## 3. 公式（权威约定，黄金测试逐条钉）
设 nav 点列 `(t_i, nav_i)`，`r_i = nav_i/nav_{i−1} − 1`（n = len−1 ≥ 1，否则整体 None；任一 nav ≤ 0 → None）：
- `span_years = (t_last − t_first).num_seconds / (365.25 × 86400)`；`bpy = n / span_years`。
- **ann_return**（CAGR，几何）= `(nav_last/nav_first)^(1/span_years) − 1`；span < 30 天 → None。
- **ann_vol** = `sample_std(r) × √bpy`（n ≥ 2；span < 30 天 → None）。
- **sharpe** = `ann_return / ann_vol`（两者 Some 且 ann_vol > 1e-12，rf=0）。
- **sortino** = `ann_return / (downside × √bpy)`，`downside = √(Σ min(r_i,0)² / n)`（全量 n 分母约定）；downside ≤ 1e-12（无负收益）→ None。
- **calmar** = `ann_return / max_drawdown`（max_drawdown > 1e-12）。
- **var95** = 升序 `sorted[idx]`，`idx = max(⌈0.05·n⌉ − 1, 0)`；**cvar95** = `mean(sorted[..=idx])`。VaR 族不依赖年化，恒可算。
- **t_stat**（SignalStat）= `mean / (sample_std / √n)`（n ≥ 2 且 std > 1e-12，否则 None）。

## 4. 架构
- `src/report/risk.rs`（新）：`RiskMetrics`（Serialize+Deserialize+Debug+Clone+PartialEq）+ `pub fn risk_metrics(nav: &[(NaiveDateTime, f64)], max_drawdown: f64) -> Option<RiskMetrics>`。
- `SimReport.risk: Option<RiskMetrics>`：run_sim 用内存 step records 的 `(t, nav)`（与是否写 traces 无关；决策点 < 2 → None）。
- `PortfolioReport.risk`：holdings 的 `(t, nav)`（调仓点粒度）。
- `SignalStat.t_stat: Option<f64>`（serde default）：在 SignalStat 构造处统一计算（硬/软/walk-forward 逐折自动获得）。
- print_sim_summary / print_portfolio_summary / print_summary(t_stat 行) + render_sim_html / render_portfolio_html headline（格式 `{:.2}`，None → "—"）。

## 5. 测试
- 黄金闭式：恒定 r=0.1%、253 点恰跨一年 → ann_return = 1.001²⁵² − 1、ann_vol≈0 → sharpe None；交替 +1%/−0.5% → vol/sortino 按 §3 公式在测试内显式重算断言（钉年化接线）；VaR n=20 已知集合 idx=0 手算；span 20 天 → 年化族 None 而 var95 有值；nav 单点/含 0 → 整体 None。
- t_stat：已知样本手算；n=1 → None。
- 兼容：旧 sim/portfolio JSON（无 risk 字段）反序列化成功。
- e2e：run_sim/run_portfolio 报告含 risk（Some）；HTML 含 "Sharpe"。
- 真数据 smoke：神华 qfq `--sim` → Sharpe/Calmar 数值合理（有限、量级正常）。
- 文档：cli-reference 指标表 + README 一句；公式约定指向本 spec。

## 6. 里程碑
- **T1** `risk.rs` 纯函数 + 黄金闭式测试。
- **T2** 挂载（SimReport/PortfolioReport/SignalStat.t_stat）+ print/HTML 行 + 兼容测试。
- **T3** e2e + 文档 + 真数据 smoke。
