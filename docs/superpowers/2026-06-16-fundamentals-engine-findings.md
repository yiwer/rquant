# 基本面进引擎（子项①）发现（诚实记录）

- 日期：2026-06-16
- 范围：子项① = 基本面 point-in-time 进引擎 + 在现有 20 上验证。spec/plan = docs/superpowers/{specs/2026-06-15-fundamentals-engine-design.md, plans/2026-06-16-fundamentals-engine.md}。
- 结论一句话：**机制端到端打通（akshare 管线 + `fund.<col>` DSL + 公告日时点 + 全市场 5615 只财务 CSV）；20 上的基本面 IC 弱（未达 F-1）但符号多为正，且 20 只横截面统计功效不足——真正的检验属于子项② 的 2000 universe。**

## 1. 机制（已建成、测试、可复用）

- **数据管线**：`scripts/fetch_fundamentals.py`（akshare `stock_yjbb_em` 按季 → 全市场逐股 `data/fundamentals/<sym>.csv`，**公告日（最新公告日期）为时点锚**）。33 季全成功，**5615 只**。
- **引擎**：`src/data/fundamentals.rs`（FundamentalSeries + as-of-t）；`UniverseEntry` 第4列 `fundamentals`；`build_context` 加 `fundamentals` 参 → `Context.fundamentals` as-of-t 快照；DSL **`fund.<col>`** 命名空间（eval + 树加载校验都接受；首报前 NaN 弃权；缺列弃权不报错）。PE/PB 派生（`close/fund.eps`、`close/fund.bps`）。
- **point-in-time 铁律**：决策 t 只见公告日 ≤ t 的财报；首报前弃权——引擎层强制（同 aux time≤t 闸）。单测钉死。
- 门：cargo test --workspace 362 单元 + 89 桥接 + 22 e2e 零失败、clippy --workspace 干净。

## 2. 20 上的 IC 验证（诚实，point-in-time，gross）

`rquant factor --universe universe_20_fund.csv`（sample20/horizon20/layers5），n_sample=90、各因子 n_skipped=4（首报前的时点弃权边界，正常）：

| 因子 | RankIC 均值 | RankICIR | F-1(|RankIC|>0.03 ∧ |ICIR|>0.3) |
|---|---|---|---|
| roe (净资产收益率) | +0.0206 | 0.075 | ✗ |
| np_yoy (净利润同比) | +0.0196 | 0.090 | ✗ |
| pb (close/bps) | +0.0231 | 0.074 | ✗ |
| pe (close/eps) | +0.0163 | 0.053 | ✗ |
| gross_margin | −0.0015 | −0.006 | ✗（无信号）|

**判读**：无一达 F-1（RankIC 都在 0.02 量级 < 0.03，ICIR 0.05–0.09 远 < 0.3）。但 **roe/np_yoy/pe/pb 符号均为弱正**（高 ROE/成长 → 高前瞻，经济上合理；PB 正略反价值但弱）；gross_margin ≈0。

**关键诚实边界**：**20 只横截面统计功效严重不足**（每期仅 20 个点算 RankIC，噪声大）——这与之前选股弧线反复点明的"20 只横截面偏薄"同根。**这不是证伪，是不结论**：基本面的真正 IC 检验需要子项② 的 ~2000 只宽横截面（届时 RankIC 才有统计意义）。弱正符号是"有微弱信号、20 只测不出显著性"的表现。

## 3. 数据特征（诚实）

- akshare yjbb 最早可得**公告日 ~2019-04**（2018 季度缺，数据源限制非脚本 bug）→ 2019 前 `fund.*` 弃权（point-in-time 正确，只是覆盖从 2019 起）。
- 重述 = 最新值（罕见，已声明）。
- 幸存者偏差：财务 CSV 含部分已退市股则部分缓解；正式处理留子项②（universe 须含退市股 + 声明）。

## 4. 前瞻（子项②/③）

- **子项② 扩到 2000**：全市场 OHLCV 批量拉 + 2000 universe 清单 + 幸存者处理 → 在宽横截面上测基本面 IC（统计功效）+ 基本面×技术选股。这是检验基本面是否真有 edge 的正确场子。
- **子项③ 方法学**：基本面（优质过滤/排序）× 技术（动量倾斜）融合选股 + 验证（point-in-time + 跨 regime + 幸存者诚实闸）。
- 机制就绪：`fund.` 通道 + 全市场财务数据已在位，子项②/③ 直接复用。
