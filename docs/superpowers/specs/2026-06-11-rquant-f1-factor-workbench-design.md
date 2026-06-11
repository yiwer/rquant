# rquant：F-1 — 因子检验工作台（factor 子命令）— 设计文档

- **日期**：2026-06-11
- **状态**：设计已确认（写 spec + 计划）
- **关联**：master `0376aac`。成熟度差距分析 F-1（研究深度层第一条）：现状只能"把因子写进树整树回测"，缺单因子检验循环（检验 → 入库 → 组合）。

---

## 1. 目标与非目标

### 目标
1. `rquant factor --universe u.csv --factor "name=expr"(可重复) --sample K --horizon H --layers Q --warmup --window --out report.json [--html out.html]`：横截面单/多因子检验。
2. 每因子：IC/RankIC 序列与汇总（mean/std/ICIR/t/正占比）、IC 衰减阶梯、Q 分层回测（层年化 + Top−Bottom 价差 + 单调性）、多因子横截面相关性矩阵。
3. print 摘要 + 自包含 HTML（衰减折线/分层条形/价差净值/相关表）。

### 非目标（YAGNI）
- LLM 因子（纯量化 DSL）；中性化/去极值/z-score；单标的时序 IC；分层交易成本；IC 显著性 bootstrap（t 值已给）。

## 2. 锁定决策
| # | 维度 | 选定 |
|---|---|---|
| 1 | 口径 | 仅横截面（universe 必填）|
| 2 | 输入 | `--factor "name=expr"` 可重复（name 唯一、非保留名；expr 经 DSL parse，加载期校验）|
| 3 | 收益 | `forward_return` **gross 无成本**（成本污染因子信号；文档注明）|
| 4 | 方向 | spread 带符号（负 = 反向因子，同样有效，判读标准里写明）|

## 3. 公式与约定（权威，黄金测试逐条钉）
- **采样**：复用 universe/公共时间线/新鲜度；采样点 = `timeline[warmup], [warmup+K], …`；每点每新鲜标的 `eval_scalar(expr)`（非有限 → 出局）+ `forward_return(bars, i, h, Long).gross`（i = 该标的恰在 t 的 bar 索引；None → 出局）。有效对 < `max(Q, 5)` → 该期跳过。
- **平均秩**（并列取平均，1 起）：`[10,20,20,30] → [1, 2.5, 2.5, 4]`。
- **Pearson**：n ≥ 2 且两侧方差 > 1e-12，否则 None。**Spearman** = Pearson(秩, 秩)。
- **IC 汇总**：对逐期 IC 序列取 mean/sample_std/ICIR=mean/std/t（复用 `risk::t_stat`）/正占比；RankIC 同款。序列空 → 整因子各项 None/0 期。
- **IC 衰减**：horizons = dedup{max(H/4,1), max(H/2,1), H, 2H, 4H} 升序；每档 mean RankIC（逐档独立采样有效性）。
- **分层**：每期按因子值**升序**分 Q 连续层，层大小 `n/Q`，前 `n%Q` 层 +1（n=11,Q=5 → [3,2,2,2,2]）；层收益 = 成员 gross 均值；层净值连乘；层年化经 `risk_metrics`（nav 点列用采样点时间戳）。**spread**：`r_top − r_bottom`（最高因子层 − 最低）连乘净值 → total/ann/Sharpe（risk_metrics）。**单调性** = Spearman(层序号, 层期均收益)。
- **相关性矩阵**（≥2 因子）：逐期对每因子对在共同有效标的上做 Spearman（≥5 个共同点才计入）→ 各期平均；对角恒 1。

## 4. 架构
```
新增 src/factor/stats.rs   # average_ranks / pearson / spearman 纯函数
新增 src/factor/mod.rs     # FactorConfig/FactorSpecItem/run_factor/FactorReport/FactorStats/LayerStats/print_factor_summary
改动 src/report/viz.rs     # render_factor_html（衰减 multi_line_chart 多因子叠加 / 分层年化 bar_chart / spread 净值 line_chart / 相关性 HTML 表）
改动 src/cli/mod.rs        # Cmd::Factor（--factor 解析 name=expr、name 唯一/非空校验）
改动 src/lib.rs 或 main 模块树  # + pub mod factor;
```
- `run_factor(cfg) -> Result<FactorReport>`：同步纯量化（无 LLM/async 不必要——但 universe 加载与 DSL eval 均同步，直接同步 fn；CLI 在 tokio 下直接调用）。
- `FactorStats` 全字段 serde；汇总比率除零 → None（沿用 F-4 哲学：拒绝假数字）。
- 报告 meta：universe 大小、采样点数、跳过期数（< 阈值）、horizon/sample/layers 参数回显。

## 5. 判读标准（文档必写，docs/factor-guide 小节或 cli-reference 内）
- |RankIC| > 0.03 且 |ICIR| > 0.3 → 值得入树；分层单调（|单调性| > 0.8）且 |spread Sharpe| > 1 → 强因子；负值 = 反向使用。
- 两因子相关 > 0.7 → 冗余，留 ICIR 高者。
- gross 口径提醒：入树后必须经 backtest/sim 含成本复检。

## 6. 测试
- stats 黄金：平均秩并列例；pearson ±1/None；spearman 非线性单调=1。
- 分层切分 [3,2,2,2,2]；衰减阶梯 H=4 → {1,2,4,8,16}。
- 合成黄金横截面：6 标的 × 多期，因子值 = 未来收益的单调函数 → IC=RankIC≈1、分层完全单调（单调性=1）、spread>0；因子取反 → 全部反号。
- 反向因子 corr：B=−A → 矩阵 −1。
- 有效样本不足期被跳过（meta 计数）。
- e2e：合成 universe 全链路 JSON/HTML；真数据 smoke：4 真股 qfq 上 mom20 vs rsi14（数字记录）。
- 文档：cli-reference（factor 子命令全表 + 判读标准 + gross 提醒）、README 一节。

## 7. 里程碑
- **T1** `factor/stats.rs` 纯函数 + 黄金。
- **T2** 采样循环（因子值/前瞻收益矩阵收集，含跳过规则）+ 合成黄金。
- **T3** 聚合（IC 汇总/衰减/分层/相关性）+ 反号对称测试。
- **T4** CLI + print + `render_factor_html`。
- **T5** e2e + 文档（判读标准）+ 真数据 smoke。
