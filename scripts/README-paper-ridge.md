# 「去相关岭组合」前向纸面册 (paper_ridge.py)

ridge-on-gauss 多因子复合（**去相关岭组合**）通过了全部可回测验证关后，唯一剩下的诚实检验是
**前向纸面盘**：在今天锁定真实的周频 top-3 选股，随时间推移结算其盈亏。**纯纸面，不涉真钱，不
触碰已冻结的实盘部署（价值净利双核 月频 top-3）。**

## 它凭什么上纸面盘

| 验证关 | 结果 |
|---|---|
| placebo（置换训练标签）| OOS 转负 → 无泄露 |
| OOS rank-IC | ~0.066 / ICIR~0.5（超任一单因子）|
| 权重合理性 | 小而分散、无前视、无突出单因子 |
| vetted-harness 控制 | 复现等权 membership +0.042 |
| embargo（砍训练尾4周）| +0.222 → +0.289（去边界泄露反升）|
| ① 跨 6 regime（含 2020 疫情）| 6/6 正、去 2025 仍 5/5 |
| ② 成本压测 | 50bp 仍 +0.131、换手仅 0.33/调仓 |

详见 `docs/superpowers/2026-06-23-*-findings.md` 与 `scripts/eval_ridge*.py`。

## 诚实前向口径（核心）

1. **权重冻结于 inception**：`fit_ridge` 在**全部已标注历史**（面板起点 .. 最后一个有
   `fwd_ret_5d` 的日期）上拟合一次，迟滞 delta 在同一训练切片上选定（此刻无 OOS 可窥）。
   结果写入 `data/factor_panel/paper_ridge_weights.json`，**之后不再静默重训**。
2. **纸面册只含 `train_hi` 之后的周频日期**——每个这样的日期都是真正的样本外。
3. **选股在下单时锁定**（status=open）：用当日合格截面 + 冻结权重打分。当周尚无
   `fwd_ret_5d`（未来未发生）——这正是前向的本质。盈亏在标的的 `fwd_ret_5d` 可得后才用
   **锁定的标的**结算（status=closed）。`journal.csv` 是选股的唯一真相源。

打分与 `eval_ridge.backtest_ridge` 逐字一致：`norm_gauss(因子矩阵) @ w`，对上周持仓加
`+delta`，按 (分降序, 代码升序) 取 top-3；硬闸 `_eligible`（非ST ∧ roe>0 ∧ bm>0 ∧ 流动性≥5e7）。

## 每周怎么跑

```bash
# 1) 数据前向推进（baostock 计划任务已跨会话自愈抓取日线）；如需手动：见 scripts/fetch_*.
# 2) 重建周频因子面板到最新：
python scripts/build_factor_matrix.py
# 3) 推进纸面册（首次运行自动冻结权重）：
python scripts/paper_ridge.py
```

子命令：

| 命令 | 作用 |
|---|---|
| `python scripts/paper_ridge.py` | 推进册：开新仓 / 结算到期 / 写盘 / 打印状态 |
| `python scripts/paper_ridge.py --status` | 只读打印当前册，不写盘 |
| `python scripts/paper_ridge.py --retrain` | 按今天最新已标注数据**重冻**权重（= 开一本新册，inception 前移）|

## 产物

- `scripts/paper_ridge.py` — harness（纯逻辑 `select_picks`/`realize_position`/`advance_journal`
  + I/O + 打印）。单测 `scripts/test_paper_ridge.py`（合成数据，10 例）。
- `data/factor_panel/paper_ridge_weights.json` — 冻结权重 + 元数据（train 区间 / delta / 因子序）。
  **钉住 inception，勿删**；要前移训练截止须显式 `--retrain`。
- `data/factor_panel/paper_ridge_journal.csv` — 纸面册：每周一行
  `date,status,picks,prev_picks,turnover,gross_ret,net_ret`。每周推进后建议提交，形成审计轨迹。

二者可由（冻结权重 + 面板）确定性重建，故体积小、可纳入版本控制；`factors.csv` 体量大不提交。

## 与实盘部署的关系

本纸面册与**已冻结的实盘冠军**（价值净利双核 月频 top-3 + ST 闸）**完全隔离**：不同策略、不同
频率、不同代码路径。是否在前向纸面盘累计足够证据后替换/并行实盘，是用户的决定——本 harness 永远
只写纸面，不下真单、不动钱。
