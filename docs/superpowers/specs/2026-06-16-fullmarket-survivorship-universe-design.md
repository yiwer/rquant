# 全市场扩展 + survivorship-free universe + 宽截面基本面 IC 验证（子项②）· 设计文档

- 日期：2026-06-16
- 状态：设计已与用户逐节确认，待写 spec → 自审 → 用户审 → writing-plans
- 范围：大方向"基本面 + 全市场 2000"的**子项②**——把标的范围从 deep-20 扩到**全市场（~5000 + 已退市）**，以 **survivorship-free**（时点全集，含退市股活跃期）方式构造 **top-2000-按成交额-at-t** 的 universe，并在这个宽截面上**诚实检验**基本面因子的预测力（① 在 20 名上欠采样做不了的正经检验）。**不含** 子项③（完整基本面×技术选股方法学）。

## 1. 背景与大方向拆解

四阶段收敛已证：纯 OHLCV 技术信号在这组大盘上难稳健跑赢买入持有（`2026-06-15-screen-tilt-loop-findings.md`）。用户定方向：引入基本面 + 全市场扩到 ~2000。四子系统弧线，按依赖序构建：

- **① 基本面进引擎**（DONE，merged cf19391）：akshare 管线 → 逐股 point-in-time 财务 CSV；引擎 `fund.<col>` DSL 通道；在 20 上验证（RankIC ~0.02，弱正但欠采样，INCONCLUSIVE）。
- **② 全市场 + 幸存者 + 宽截面验证**（本 spec）：全市场 OHLCV 批量抓 + survivorship-free top-2000 membership 工件 + factor 支持 membership mask + 宽截面基本面 IC。
- **③ 基本面×技术 选股方法学 + 验证**（在 2000 上）。

每子项各自 spec→plan→实现。本 spec 只覆盖 ②。

## 2. 已确认决策（brainstorming）

| # | 决策 | 选择 |
|---|---|---|
| 幸存者 | universe 是现存快照还是时点全集 | **时点全集 / survivorship-free**：纳入各时点所有"当时在市"的股（含后来退市的）；membership 随 IPO 进、退市出。方法学正解。 |
| universe 口径 | 时变 membership 取哪 ~2000 | **top-2000 按成交额(流动性) at t**：每期从时点数据重算成交额排名取前 2000。可交易性 + survivorship-free 天然契合。 |
| OHLCV 源 | 全市场日线怎么抓 | **akshare Python**（`stock_zh_a_hist` qfq 逐股循环）：与 fundamentals 管线同一工具链；qfq 内置；输出逐股 `data/<sym>.csv` 与引擎现有 primary 格式一致。 |
| 架构 | 时变 membership 怎么表示/强制 | **预计算 membership 工件文件**：builder 算出每再平衡日 top-2000 在市集合 → 可复现/可审计 CSV；factor/screen 读全市场 + mask。 |
| builder/消费分工 | — | builder = Python（pandas 横截面排名，数据 prep）；强制/点时**消费在 Rust**（带测试）。双重防护：builder ≤t 排名 + 引擎 membership effective-as-of-t。 |
| 验证口径 | — | 月末再平衡；RankIC across top-2000；ICIR=mean/std；多前瞻（factor `decay_ladder`，h=20 → 5/10/20/40/80）；F-1 门槛 \|RankIC\|>0.03 ∧ \|ICIR\|>0.3（沿用 ①）。 |

**数据宽度 vs 分析宽度（关键）**：survivorship-free + top-2000-at-t ⟹ 要算"某时点 top-2000"必须先有**全市场**（含退市股）OHLCV 来排名 → **抓取宽度=全市场(~5000+退市)**，**分析宽度=每期 top-2000**。

## 3. 数据管线（Python akshare，扩展 `scripts/`）

### 3.0 Task 0 — 可行性 spike（先于全量抓取，de-risk）

全量抓取前先验证 akshare 对**退市股**的支持（survivorship-free 的命根）：

1. 退市清单接口：`ak.stock_info_sh_delist()` / `ak.stock_info_sz_delist()` 能否返回 + 是否含**退市日**字段。
2. 取一只**已知退市股**（如某 *ST 退市标的）跑 `ak.stock_zh_a_hist(symbol, adjust="qfq")`，确认能否返回其历史 OHLCV。

**spike 结果决定降级**：若退市股 OHLCV 不可得，则诚实声明残余幸存者偏差（哪段时间、多大尾部占比），写入 findings——**不假装零偏差**。spike 产出一个简短 `docs/superpowers/2026-06-16-fullmarket-akshare-spike.md`。

### 3.1 roster 抓取 → `data/universe_full.csv`

`scripts/build_roster.py`（新，或并入 fetch_ohlcv.py 的前置步）：

- 在市全清单：`ak.stock_info_a_code_name()`（返回 `code`,`name`）。
- 退市清单：`ak.stock_info_sh_delist()` + `ak.stock_info_sz_delist()`（spike 确认字段后）。
- 合并去重 → 全市场 roster（含退市股）；代码映射复用 `fetch_fundamentals.py` 的 `to_symbol`（6 位 → `sh/sz` 前缀：`60/68/9→sh`、`00/30/2→sz`；其余跳过+日志）。
- 输出 `data/universe_full.csv`，表头 `symbol,primary,context,fundamentals`，每行：
  ```
  symbol,primary,context,fundamentals
  sh600000,data/sh600000.csv,,data/fundamentals/sh600000.csv
  ```
  - **context 留空**：`read_universe_csv` 在 context 列空时回退 = primary（`universe.rs:33-37`），纯基本面因子不引用 `ctx.*`，无害。
  - fundamentals 列指向 ① 已生成的全市场逐股财务 CSV（缺失则留空 → None）。

### 3.2 OHLCV 抓取 → `data/<sym>.csv`

`scripts/fetch_ohlcv.py`（新）：

- 逐股 `ak.stock_zh_a_hist(symbol=6位code, period="daily", adjust="qfq", start_date="20180101", end_date=今)` → 转换为引擎 primary 格式 CSV（`time,open,high,low,close,volume[,amount...]`，列名/顺序对齐现有 `data/<sym>.csv`；`time` 升序）。
- **resume/增量**：若 `data/<sym>.csv` 已存在，读末行 `time`，从次日 `start_date` 续拉、追加（不重抓全量）。
- **限速**：每股之间 `time.sleep(δ)`（如 0.3~0.5s）；~5000 股分批；失败记日志续跑，不中断全量（断点可续）。
- **退市股**：按 spike 结论处理（能拉则拉，不能拉则该股缺 OHLCV → 自动不进任何月份的 top-2000，findings 声明）。
- 起始 `20180101` 对齐 ① 的 fundamentals 覆盖（yjbb 自 2018），保证 IC 测试有基本面+OHLCV 重叠期。
- **横截面单源一致性**：现有 deep-20 `data/<sym>.csv` 来自 Rust Tencent 多窗口源（DATA 子项），其 qfq 复权基准可能与 akshare 不同。同一横截面**混两源 qfq 会污染排名/IC**。故 akshare **单源重抓全市场（含 20 重叠股）**保证一致。覆盖前**先备份旧 deep-20 CSV**（如 `data/_tencent_backup/`）——既有 screener/factor 结论基于旧源，重跑会微移；plan 决定覆盖+备份 vs akshare 写隔离目录。

### 3.3 校验

复用既有 `rquant validate-data` / `data::quality`（DATA 子项已建）对抽样股做覆盖/缺口校验；roster 数值合理性（非空、time 单调）。

## 4. Membership 工件（survivorship-free 核心）

### 4.1 builder → `scripts/build_membership.py`（新，pandas）

- 读全市场 `data/*.csv` OHLCV。
- 再平衡日 = **每月末交易日**（2018-01 … 今）。对每个再平衡日 `d`：
  1. "在市"集合 = 在 `d` 当日有 bar 的股（has bar at d）。
  2. 在该集合内按 **近 20 个交易日均成交额** 降序排名，取 **top-2000**（不足 2000 则全取）。
  3. 写出 `(d, symbol)` 行。
- **point-in-time 铁律**：排名只用 `≤d` 的数据（近 20 日窗口在 `d` 及以前）；后来退市的股在其活跃月份正确进入 top-2000，退市后无 bar 自动消失。
- **量纲安全**：此处是**排名**（scale-invariant），qfq volume 的"手×100"量纲不改变排序——规避了迭代 #2 流动性闸的量纲 bug（那是**阈值**，scale-sensitive）。成交额近似 `close × volume`（常数因子对排序无影响），无需还原绝对值。
- builder 同时输出**成员并集 roster** `data/universe_membership.csv`（曾进过任一月份 top-2000 的全部 symbol，格式同 `universe_full.csv`）——供 factor 只加载这批以控内存（见 §5 内存）。

### 4.2 文件格式 → `data/membership_top2000.csv`（gitignored，可复现）

long 格式，按 `date` 升序、同日按 `symbol` 升序：
```
date,symbol
2018-01-31,sh600000
2018-01-31,sz000001
2018-02-28,sh600000
```
git-diffable、易加载、**可审计**（查"某日谁在 universe"）。

### 4.3 消费（Rust，带测试）—— `src/data/membership.rs`（新）

```rust
/// 按再平衡日升序的成员快照；effective_at 取 ≤t 的最近一期。
pub struct Membership {
    snapshots: Vec<(chrono::NaiveDate, std::collections::BTreeSet<String>)>,
}
impl Membership {
    /// 加载 long 格式 CSV（表头 date,symbol）；按 date 分组 → 升序快照。
    pub fn load_csv(path: &std::path::Path) -> crate::Result<Self> { /* ... */ }
    /// 取生效成员集 = 再平衡日 ≤ t.date() 的最近一期；无（t 早于首期）→ None。
    pub fn effective_at(&self, t: chrono::NaiveDateTime) -> Option<&std::collections::BTreeSet<String>> { /* partition_point */ }
}
```

- `effective_at` 用 `partition_point` 找 `≤ t.date()` 的最近快照（point-in-time：t 时刻只见已生效的成员名单）。
- **单测**：
  - `effective_at` 在再平衡日推进时取最近一期；t 早于首期 → None。
  - point-in-time 正确性：构造两期成员（d1 含 A、d2 含 B），断言 `t∈[d1,d2)` 只见 d1 名单。

## 5. 引擎集成 —— factor 支持 membership mask

`src/factor/mod.rs`：

- `FactorConfig` 加 `pub membership_path: Option<PathBuf>`。
- `collect_periods` 中：
  - universe 加载后：`let membership = cfg.membership_path.as_ref().map(|p| Membership::load_csv(p)).transpose()?;`
  - 采样循环每个 `t` 开头算一次：`let has_m = membership.is_some(); let eff = membership.as_ref().and_then(|m| m.effective_at(t));`
  - per-symbol 循环（现 `mod.rs:202-210`，bar 存在性检查之后）加一道闸：
    ```rust
    if has_m && eff.map_or(true, |set| !set.contains(&entry.symbol)) {
        continue; // 不在 t 时点生效成员集 → 不进该期截面
    }
    ```
    - `has_m && eff=None`（membership 提供但 t 早于首期）→ 跳过所有 → 该期空截面（universe 未定义，正确）。
    - `membership=None` → 不过滤（**行为冻结**，向后兼容 ① 的 20-name 调用）。
- 报告语义：`FactorReport.n_symbols` 仍为 universe.len()（加载规模）；每期有效截面 ≤2000 由 `n_periods`/`n_skipped` 与逐期有效对数体现（无需改 schema）。

`src/cli`（factor 子命令）：加 `--membership <PATH>` 可选参，透传到 `FactorConfig.membership_path`；缺省 None（冻结）。

**内存**：factor 一次性加载全 universe 的 primaries+contexts+funds（`mod.rs:161-174`）。全市场 ~5000 股全历史约 1~2GB。缓解：默认把传给 factor 的 universe 用 **§4.1 的成员并集** `data/universe_membership.csv`（只含曾进 top-2000 的股，~3000-4000），而非 `universe_full.csv` 全量。plan 中评估实测内存，必要时再做 symbol-chunk 流式（YAGNI，先用并集裁剪）。

## 6. 宽截面验证（② 的检验目的，D-subset proof）

```
rquant factor --universe data/universe_membership.csv \
  --membership data/membership_top2000.csv \
  --sample 20 --horizon 20 --layers 5 \
  --factor "roe=fund.roe" \
  --factor "npyoy=fund.np_yoy" \
  --factor "revyoy=fund.rev_yoy" \
  --factor "gm=fund.gross_margin" \
  --factor "pe=close/fund.eps" \
  --factor "pb=close/fund.bps"
```

- 每采样期横截面 **RankIC**（across 当期 top-2000 成员）；**ICIR**=mean/std(RankIC)；分层组合收益（Q=5）。
- **多前瞻 horizon**：`factor` 的 IC 衰减阶梯（`decay_ladder`）已自动报 h/4,h/2,h,2h,4h——设 `--horizon 20` 即 5/10/20/40/80（短到长的前瞻结构），看信号瞬时 vs 持久。若要 60 量级单独看，可另跑 `--horizon 30`（7/15/30/60/120）。
- **F-1 门槛**：\|RankIC\|>0.03 ∧ \|ICIR\|>0.3（沿用 ①）。
- **交付 = findings 文档** `docs/superpowers/2026-06-16-fullmarket-fundamental-ic-findings.md`：每因子在宽截面的 IC/ICIR/分层 + **works / inconclusive / falsified** 诚实判定。这是 ① 在 20 名（欠采样）做不了的正经检验。

## 7. 文件

| 文件 | 改动 |
|---|---|
| `scripts/build_roster.py` | 新建：在市+退市清单合并 → `data/universe_full.csv`（代码映射复用 to_symbol）|
| `scripts/fetch_ohlcv.py` | 新建：逐股 `stock_zh_a_hist` qfq → `data/<sym>.csv`；resume/增量/限速/失败续跑 |
| `scripts/build_membership.py` | 新建：月末 top-2000 按成交额（≤d 排名）→ `data/membership_top2000.csv` + 成员并集 `data/universe_membership.csv` |
| `src/data/membership.rs` | 新建：`Membership` + `load_csv` + `effective_at`（partition_point）+ 单测（point-in-time）|
| `src/data/mod.rs` | 挂 `pub mod membership;` |
| `src/factor/mod.rs` | `FactorConfig.membership_path`；`collect_periods` 加载 + per-symbol mask 闸；单测（mask 生效 + 无-membership 冻结）|
| `src/cli`（factor 子命令）| `--membership <PATH>` 可选参透传 |
| `data/universe_full.csv` | 生成：全市场 roster（含退市）|
| `data/universe_membership.csv` | 生成：成员并集 roster（factor 加载用，控内存）|
| `data/membership_top2000.csv` | 生成：long 格式成员表（gitignored）|
| `.gitignore` | 确认 `data/*.csv` 已忽略（含新生成物）|
| `docs/cli-reference.md` | factor `--membership` 说明 + universe_full/membership 文件格式 |
| `docs/superpowers/2026-06-16-fullmarket-akshare-spike.md` | spike 结论（退市数据可得性 + 降级声明）|
| `docs/superpowers/2026-06-16-fullmarket-fundamental-ic-findings.md` | 宽截面 IC 验证 findings（诚实判定）|
| 闸 | `cargo test --workspace` + `cargo clippy --workspace --all-targets`（factor 是引擎公共路径；membership 是新 data 模块）+ membership point-in-time 单测 + factor 无-membership 冻结回归 |

## 8. 诚实边界（非目标）

- 子项② = 全市场数据基建 + survivorship-free membership + factor mask + **宽截面 IC 验证**；**不**做完整选股方法学（③）、不出新交易信号。
- **② 的成功 = 数据基建打通 + 幸存者正确 + IC 被诚实测量并归档**。works / inconclusive / falsified **都是有效交付**；**不**以"找到能用的因子"为完成门控，**不为好看数字调参**（§5.3）。
- **幸存者偏差**：membership 含退市股活跃期 → survivorship-free；**spike 若证 akshare 退市 OHLCV 不可得，则声明残余偏差**（哪段、多大尾部），不假装零偏差。
- **point-in-time 双闸**：membership 排名只用 `≤d` 数据 + 引擎 `effective_at(t)` 只见 `≤t` 名单 + `fund.as_of(t)` 只见 `≤t` 财报。
- **量纲**：membership 排名 scale-invariant（不复发迭代 #2 阈值量纲 bug）。
- **内存**：全市场加载约 1~2GB；用成员并集裁剪缓解；实测后必要时再流式（YAGNI）。
- **行为冻结**：`membership_path=None` 时 factor 与改造前逐字一致（① 的 20-name 调用不受影响）。
- **单源一致性**：全市场 OHLCV 单源 akshare qfq；deep-20 旧 Tencent CSV 覆盖前备份（既有结论基于旧源，重跑微移）。
- akshare 是数据管线依赖；引擎仍纯 Rust，脚本独立（同 fetch_fundamentals.py 模式）。
- `FactorConfig`/新 `data::membership` 是引擎公共 API → 闸必 `--workspace`。
