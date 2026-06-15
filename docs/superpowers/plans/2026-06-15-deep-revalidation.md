# 深数据重验（4 棵树跨 regime）实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在深日线数据（2018-2026）+ 扩容 20 标的上重验 4 棵纯量化决策树，用 `rquant eval` 出机器裁决 + 强度树牛/熊切片，诚实回答"上次证伪是数据局限还是真无 edge"。

**Architecture:** 纯执行 + 文档，**零 Rust 改码**——全程复用已建基建（`fetch --from` 深拉 / `validate-data` / `optimize --auto-extend` / `eval` 五门槛 / `portfolio`）。触发树拓宽网格重新寻优（eval 五门槛做过拟合防线）；强度树组合回测 + regime 时间切片。

**Tech Stack:** `target/release/rquant.exe`（已含全部基建）；数据 `data/*.csv`（深日线 qfq，gitignored）；中间产物 `tmps/`（gitignored）。设计：`docs/superpowers/specs/2026-06-15-deep-revalidation-design.md`。

**重要前提**：本弧线不改 Rust 代码。每个实验任务＝跑命令 + 读 JSON + 判读 + 写报告节。证伪是有效产出，不强行调参凑数。

---

## 文件结构

| 文件 | 责任 | 改动 |
|---|---|---|
| `data/<新10只>.csv` | 新标的深日线 | 新建（gitignored，脚本拉取） |
| `data/fetch_deep.cmd` | 批拉脚本 | 扩到 20 标的 |
| `data/universe_20.csv` | 强度树横截面 universe | 新建（提交） |
| `docs/superpowers/2026-06-14-data-expansion-coverage.md` | 覆盖报告 | 追加新 10 只 |
| `docs/superpowers/2026-06-15-deep-revalidation.md` | 重验对比报告 | 新建 |
| `tmps/wfo_<tree>_<sym>.json`、`tmps/pf_*.json` | WFO/组合产物 | 新建（gitignored） |

---

## Task 1: 扩 10 标的深数据 + 校验 + universe_20

**Files:**
- Modify: `data/fetch_deep.cmd`（10→20 标的）
- Create: `data/universe_20.csv`、`data/<新10只>.csv`（gitignored）
- Modify: `docs/superpowers/2026-06-14-data-expansion-coverage.md`（追加新 10）

> 联网执行。需 `web.ifzq.gtimg.cn`。沙箱挡网络则对联网命令设 dangerouslyDisableSandbox。先 `cargo build --release`（应无改动、直接复用既有二进制）。

- [ ] **Step 1: 扩脚本到 20 标的**

`data/fetch_deep.cmd` 的 `for %%S in (...)` 列表追加新 10 只：

```
sz002415 sz000002 sh600104 sz300059 sh600887 sh600309 sh601899 sz002475 sz002714 sh601012
```

（最终 20 只一行；保持 `--scale 240 --adjust qfq --from %FROM% --out data\%%S.csv`，FROM=2018-01-01）。

- [ ] **Step 2: 拉新 10 只**

只拉新增（避免重拉原 10）：逐条跑或临时脚本：

```
for %%S in (sz002415 sz000002 sh600104 sz300059 sh600887 sh600309 sh601899 sz002475 sz002714 sh601012) do target\release\rquant.exe fetch --symbol %%S --scale 240 --adjust qfq --from 2018-01-01 --out data\%%S.csv
```

逐行确认无报错；留意 `[rquant] trimmed N ...` 日志（高成长股立讯/隆基/牧原/东财可能有前导 qfq 缩放被 trim）。

- [ ] **Step 3: 校验新 10 只**

```
target\release\rquant.exe validate-data --csv data/sz002415.csv --csv data/sz000002.csv --csv data/sh600104.csv --csv data/sz300059.csv --csv data/sh600887.csv --csv data/sh600309.csv --csv data/sh601899.csv --csv data/sz002475.csv --csv data/sz002714.csv --csv data/sh601012.csv
```

记录每只 bars/coverage/monotonic/max|ret|/jumps/gaps/trim。可疑跳空逐条排查（IPO 涨停/qfq 缩放/真实行情）。退出码非 0 不阻断——记入覆盖报告。

- [ ] **Step 4: 建 universe_20.csv**

`data/universe_20.csv`，格式与既有 universe CSV 一致（看 `deploy/universe_10.csv` 的列与字典序约定），20 只（原 10 + 新 10）按字典序，主数据路径指 `data/<sym>.csv`、context 列留空或同既有约定。

- [ ] **Step 5: 覆盖报告追加新 10**

`docs/superpowers/2026-06-14-data-expansion-coverage.md` 的每标的表追加新 10 行（Step 3 实测），regime 标注（新标的起始日期是否 2018-01）。

- [ ] **Step 6: 提交**

```bash
git add data/fetch_deep.cmd data/universe_20.csv docs/superpowers/2026-06-14-data-expansion-coverage.md
git commit -m "data(reval): expand universe to 20 symbols (deep daily qfq) + validate"
```

> data/*.csv 不提交（gitignore）。交付 = 20 只本地深数据 + 脚本 + universe_20 + 覆盖报告追加。

---

## Task 2: 树1 均值回归 深 WFO + eval 裁决

**Files:**
- 产出（gitignore）：`tmps/wfo_mr_<sym>.json` ×20、`tmps/verdict_mr.json`

- [ ] **Step 1: 20 标的深 WFO（拓宽网格）**

对 20 只逐标的跑（拓宽：k_dev 两端各延、rsi_lo 两端各延、stop_n 两端各延；折数 5 利用深数据；auto-extend 4）：

```
target\release\rquant.exe optimize --tree examples/mean_reversion_1.yaml --primary data/<sym>.csv --context data/<sym>.csv --grid "k_dev=1.0,1.5,2.0,2.5,3.0" --grid "rsi_lo=20,25,30,35,40" --grid "stop_n=1.0,1.5,2.0,2.5,3.0" --folds 5 --sim --auto-extend 4 --max-combos 500 --out tmps/wfo_mr_<sym>.json
```

（125 combos × 5 折 × 20 标的；--context 用主数据自身占位，与上一弧线一致——均值回归树不依赖 context。逐标的替换 `<sym>`。）

- [ ] **Step 2: eval 五门槛裁决**

```
target\release\rquant.exe eval --reports tmps/wfo_mr_sh600030.json --reports tmps/wfo_mr_sh600036.json [...全部20个...] --name mean_reversion --out tmps/verdict_mr.json
```

记录 Verdict（certified、逐门槛 status/value/threshold/note、failed_gates）+ 退出码。

- [ ] **Step 3: 判读**

对照五门槛裁决 + 与上一弧线浅样本结局（树1 浅样本=无 edge）：深数据 + 拓宽网格后是否仍未认证？是数据局限被解除后真无 edge，还是有改观？**注明网格+深度双变混淆**。结论 4 选 1（认证/无edge/regime/数据局限已解除仍无edge）。记入待写报告的素材（commit 在 Task 6 统一，或本任务无代码改动则跳过提交）。

> 本任务无源码/受控文件改动（产物在 gitignored tmps/）——无需 git 提交；判读数字交 Task 6 写报告。

---

## Task 3: 树2 Donchian 深 WFO + eval 裁决

**Files:**
- 产出（gitignore）：`tmps/wfo_dc_<sym>.json` ×20、`tmps/verdict_dc.json`

- [ ] **Step 1: 20 标的深 WFO（拓宽网格）**

```
target\release\rquant.exe optimize --tree examples/donchian_breakout_1.yaml --primary data/<sym>.csv --context data/<sym>.csv --grid "n_break=15,20,40,55,70" --grid "vol_mult=1.0,1.2,1.5,2.0,2.5" --grid "chand_n=2.0,2.5,3.0,3.5,4.0" --grid "s1_on=0,1" --folds 5 --sim --auto-extend 4 --max-combos 500 --out tmps/wfo_dc_<sym>.json
```

（250 combos × 5 折 × 20 标的。逐标的替换 `<sym>`。）

- [ ] **Step 2: eval 裁决**

```
target\release\rquant.exe eval --reports tmps/wfo_dc_<全20>.json --name donchian --out tmps/verdict_dc.json
```

记录 Verdict + 退出码。

- [ ] **Step 3: 判读**

对照上一弧线浅样本结局（树2=regime 依赖 + 数据局限）：**深数据正是为解除"突破信号过稀"的数据局限**——折内 bar 从 ~137 增至 ~400，IS 是否终于能区分参数？退化/漂移/广度门是否改善？这是本弧线最该改观的树（数据局限曾是主因）。结论 4 选 1 + 双变混淆注明。素材交 Task 6。

> 无受控文件改动，无需提交。

---

## Task 4: 树4 均线多头 深 WFO + eval 裁决

**Files:**
- 产出（gitignore）：`tmps/wfo_ma_<sym>.json` ×20、`tmps/verdict_ma.json`

- [ ] **Step 1: 20 标的深 WFO（拓宽网格）**

```
target\release\rquant.exe optimize --tree examples/ma_stack_1.yaml --primary data/<sym>.csv --context data/<sym>.csv --grid "n_s=30,40,55,60,90,120" --grid "chand_n=2.0,2.5,3.0,3.5,4.0" --folds 5 --sim --auto-extend 4 --max-combos 500 --out tmps/wfo_ma_<sym>.json
```

（30 combos × 5 折 × 20 标的；n_f/n_m 固定不入网格，避免耦合三元组。逐标的替换 `<sym>`。）

- [ ] **Step 2: eval 裁决**

```
target\release\rquant.exe eval --reports tmps/wfo_ma_<全20>.json --name ma_stack --out tmps/verdict_ma.json
```

记录 Verdict + 退出码。

- [ ] **Step 3: 判读**

对照上一弧线浅样本结局（树4=regime 依赖/未认证，Fold4 0/10 正）：深数据含 2018 熊/2020 暴跌/2022 回调——均线多头在多个真实熊市的表现？是否仍 regime 依赖、还是跨周期有持续 edge？退化/内部最优门（深数据 + auto-extend）改善否？结论 4 选 1 + 双变混淆。素材交 Task 6。

> 无受控文件改动，无需提交。

---

## Task 5: 树3 强度 v2 深组合回测 + 牛/熊切片

**Files:**
- 产出（gitignore）：`tmps/pf_full.json`、`tmps/pf_<regime>.json`、`tmps/pf_*_no601088.json`、`tmps/slice_*` 切片 CSV

- [ ] **Step 1: 全期组合回测（深 20 universe）**

```
target\release\rquant.exe portfolio --tree examples/strength_portfolio_2.yaml --universe data/universe_20.csv --top 3 --rebalance 5 --soft --warmup 100 --window 100 --out tmps/pf_full.json
```

记录全期超额（vs 等权基准）/Sharpe/MDD/调仓次数。

- [ ] **Step 2: regime 时间切片**

定义 4 切片日期段，对 universe 中每只 `data/<sym>.csv` 按日期前缀过滤行生成切片 CSV（保留表头），建对应切片 universe，逐切片跑 portfolio：

- 熊：`2018` 段（2018-01-01..2018-12-31）→ `tmps/pf_2018bear.json`
- 牛：`2019-2021` 段 → `tmps/pf_1921bull.json`
- 压力：`2022` 段（2022-01-01..2022-12-31）→ `tmps/pf_2022corr.json`
- 近期：`2023-01..2026-06` 段 → `tmps/pf_recent.json`

过滤命令模式（PowerShell 例，按日期前缀切；表头保留）：

```
Get-Content data\<sym>.csv | Where-Object { $_ -match '^time' -or $_ -match '^2018-' } | Set-Content tmps\slice2018_<sym>.csv
```

（每切片建 `tmps/universe_<regime>.csv` 指向切片 CSV，再 portfolio。warmup 在短切片可能吃掉过多——2018 单年 ~240 bar，warmup 100 后剩 ~140，可接受；若切片太短报错则调小 --warmup 至 60 并注明。）

每切片记超额/Sharpe/MDD。**关键：熊/压力切片（2018、2022）超额是否仍为正**。

- [ ] **Step 3: top×reb 敏感性矩阵**

全期对 top∈{2,3,5} × rebalance∈{3,5,10} 跑 9 格（仿 PQ-7），记超额/Sharpe，确认默认 top3/reb5 非尖峰、面整体为正。

- [ ] **Step 4: sh601088 含/不含稳健性**

对 2018-2021 段（sh601088 pre-2022 放大收益期）跑两版组合：含 sh601088（20 只）vs 不含（19 只 universe）→ 比超额/Sharpe 差异，量化 sh601088 放大收益对横截面选股的扭曲。如实记。

- [ ] **Step 5: 判读**

强度树是唯一有改进者——深数据跨真实熊市后：edge 扛熊否（熊切片超额正负）？全期 vs 上一弧线浅牛市样本（超额 +75.2pp/Sharpe 1.43）如何变化？敏感面是否仍全正无尖峰？sh601088 扭曲多大？结局判定（改进扛熊/仅牛市/退化）。素材交 Task 6。

> 切片 CSV 与 pf JSON 均在 gitignored tmps/——无受控文件改动，无需提交。

---

## Task 6: 对比报告 + 收尾

**Files:**
- Create: `docs/superpowers/2026-06-15-deep-revalidation.md`

- [ ] **Step 1: 写对比报告**

`docs/superpowers/2026-06-15-deep-revalidation.md`，用 Task 2-5 实测：
- **每触发树（1/2/4）**：深 WFO eval 五门槛裁决表（逐门槛 value/threshold/status）+ 与上一弧线浅样本结局对照 + 结局判定（4 选 1）+ **网格+深度双变混淆注明**。
- **强度树 3**：全期指标 + 4 regime 切片表（超额/Sharpe/MDD，**熊切片是否正**）+ top×reb 敏感面 + sh601088 含/不含差异 + 结局。
- **跨树综合**：深跨-regime 数据 + 自动 eval 揭示了什么？哪些证伪被解除、哪些坐实？强度树扛熊否？
- **诚实边界**：仅日线、20 只幸存者、网格+深度双变、强度树无折叠 WFO、sh601088/sz300750 伪影、不导出部署、**再次证伪即有效结论**。

- [ ] **Step 2: 提交报告**

```bash
git add docs/superpowers/2026-06-15-deep-revalidation.md
git commit -m "docs(reval): deep cross-regime re-validation report (4 trees, 20 symbols)"
```

- [ ] **Step 3: 收尾闸（零改码确认）**

本弧线零 Rust 改码，但确认仓库仍健康：

```
cargo test
cargo clippy --all-targets
```

Expected: 全绿、零警告（与合并前一致——无代码改动）。若有异常说明非本弧线引入。

- [ ] **Step 4: 最终确认**

`git status --porcelain` 确认无遗漏受控文件、无 data/*.csv 或 tmps/ 误入暂存。

---

## Self-Review（写计划后自查）

**Spec 覆盖**：扩 20 标的 + 拉取校验 + universe_20（§3）→ Task 1；触发树 1/2/4 拓宽网格 + auto-extend + eval（§4）→ Task 2/3/4；强度树组合回测 + 牛熊切片 + top×reb 敏感 + sh601088 稳健（§5）→ Task 5；对比报告 + 诚实边界（§6/§7）→ Task 6；零改码（§架构）→ Task 6 Step 3 确认。✅

**占位符扫描**：`<sym>` 是 20 标的逐个替换的循环占位（命令模式明确，非可避免占位）；切片日期段、网格值、top/reb 均具体。Task 2-5 的判读"素材交 Task 6"是有意的报告集中化（这些任务产物在 gitignored tmps/、无受控文件改动故不单独提交）。无 TBD/含糊。

**一致性**：拓宽网格基于原网格各向延档（树1 k_dev/rsi_lo/stop_n、树2 n_break/vol_mult/chand_n/s1_on、树4 n_s/chand_n），与 §4 一致；折数统一 5；eval 命令与 verdict 产物名一致（verdict_mr/dc/ma）；强度树 top3/reb5 + 9 格敏感与 §5/PQ-7 一致；20 标的列表与 §3 一致。✅

**注**：触发树 WFO 计算量大（20 标的 × 125-250 combos × 5 折 × auto-extend 重评），单棵树可能数分钟至十余分钟；sim 模式每 combo 是一次快速回测，可接受。eval 退出码非 0（未认证）是预期常态，不阻断判读。
