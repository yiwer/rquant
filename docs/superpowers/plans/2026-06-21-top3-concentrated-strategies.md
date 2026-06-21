# 三个 top-3 集中持仓策略 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把已验证的价值核心落成 3 个集中(top-3)可投资变体（S1 纯价值 / S2 价值→1h-PA / S3 价值×板块强度），诚实回测对比。

**Architecture:** 零引擎改动。S1/S3 纯配置（复用 `run_screen` 的 quality/setup/value_frac/top combine）；S2 加 1 个新 builder（k15m 重采样 1h → 复用 `build_pa_features.pa_features()` 算 PA → merge 财务）。全部用 `iterate.py` 验证（vs csi300+EW，§5.3 闸，top-3 与 top-10 双口径）。

**Tech Stack:** Python 3.13 pandas/numpy（builder + pytest）；YAML（树/配置）；现有 Rust `rquant screen` + `scripts/iterate.py`。

## Global Constraints

- **零引擎改动**：只新增 Python builder + YAML + `iterate.py` 一个 axis 项；不改 `src/`。
- **无前视**：1h-PA 滞后 1 交易日；财务公告日 as-of。
- **持仓口径**：主 **top-3**；每策略附 **top-10** 稳定性参照（同配置改 `--top`）。
- **基准**：vs **csi300（主）+ EW（参考）**，§5.3 闸（gross>0 ∧ net-OOS>0 ∧ net-Sharpe>0 ∧ be≥40bps ∧ tier2 无符号翻转）。
- **复用**：价值树 `examples/trees/screen/{value_pb,growth_revyoy,quality_gm}.yaml`、`momentum_xs.yaml`(inert)、`build_pa_features.pa_features()`、`universe_pa_sector.csv`(已存在含 sec_mom20+财务)、`iterate.py`。
- **诚实**：top-3 统计噪声大、PA/板块先验无 edge —— 证伪也是产出，不参数钓鱼；commit 显式文件+英文(`git commit -F -`)，不 push。
- **1h 定义**：每交易日按时间顺序每 **4 根 15m** 合 1 根 1h（open=首/high=max/low=min/close=末/volume=和），每日 4 根；近 100 根算 PA。

数据：`data/baostock/k15m/<sym>.csv`(time,open,high,low,close,volume,amount)；`data/baostock/kday/<sym>.csv`；`data/fundamentals/<sym>.csv`(time,roe,np_yoy,rev_yoy,gross_margin,eps,bps)；`data/baostock/universe_pa_sector.csv`(primary=kday，fundamentals 含 sec_mom20 等)。

---

### Task 1: 1h-PA 价值合并 universe builder（build_pa1h_value_universe.py）

**Files:**
- Create: `scripts/build_pa1h_value_universe.py`
- Test: `scripts/test_build_pa1h_value_universe.py`
- Modify: `scripts/iterate.py`（`AXES` dict 末尾加 `pa1hv` 项，同 `paov` 样式）

**Interfaces:**
- Consumes: `build_pa_features.pa_features(df)`（已在 master；入 df 列 time/open/high/low/close 升序 → 返回 time + pa_* 11 列）。
- Produces: `resample_1h(df15: pd.DataFrame) -> pd.DataFrame`（入 15m bars 列 time/open/high/low/close/volume → 出 1h bars 同列，每日每 4 根合 1）；`merge_one(sym)->bool` 写 `data/baostock/pa1h_value_merged/<sym>.csv`(time + pa_* + 6 财务，PA 滞后1日、财务 as-of)；`data/baostock/universe_pa1h_value.csv`(symbol,primary=kday,context,fundamentals)；iterate `pa1hv` 轴。

- [ ] **Step 1: 写失败测试** `scripts/test_build_pa1h_value_universe.py`

```python
#!/usr/bin/env python3
"""build_pa1h_value_universe 单测：1h 重采样(每4根15m合1) + 财务 as-of merge。
跑：python -m pytest scripts/test_build_pa1h_value_universe.py -q"""
import pandas as pd
from build_pa1h_value_universe import resample_1h, merge_frames


def test_resample_4x15m_to_1h():
    # 一日 8 根 15m → 2 根 1h；每 4 根：open=首 high=max low=min close=末 vol=和
    t = pd.date_range("2021-01-04 09:45", periods=8, freq="15min")
    df = pd.DataFrame({"time": t,
                       "open":  [10, 11, 12, 13, 20, 21, 22, 23],
                       "high":  [10.5, 11.5, 12.5, 13.5, 20.5, 21.5, 22.5, 23.5],
                       "low":   [9.5, 10.5, 11.5, 12.5, 19.5, 20.5, 21.5, 22.5],
                       "close": [11, 12, 13, 14, 21, 22, 23, 24],
                       "volume":[1, 2, 3, 4, 5, 6, 7, 8]})
    h = resample_1h(df).reset_index(drop=True)
    assert len(h) == 2
    assert h.loc[0, "open"] == 10 and h.loc[0, "close"] == 14
    assert h.loc[0, "high"] == 13.5 and h.loc[0, "low"] == 9.5
    assert h.loc[0, "volume"] == 10            # 1+2+3+4
    assert h.loc[1, "open"] == 20 and h.loc[1, "close"] == 24 and h.loc[1, "volume"] == 26


def test_merge_asof_financials_no_lookahead():
    pa = pd.DataFrame({"time": pd.to_datetime(["2020-03-02", "2020-03-03"]),
                       "pa_ema20": [0.01, 0.02]})
    fin = pd.DataFrame({"time": pd.to_datetime(["2019-10-31", "2020-04-30"]),
                        "bps": [5.0, 6.0]})
    m = merge_frames(pa, fin)
    assert list(m["pa_ema20"]) == [0.01, 0.02]
    assert list(m["bps"]) == [5.0, 5.0]        # 3月用2019Q3(≤t)，不前视2020-04-30
```

- [ ] **Step 2: 跑测试确认失败**

Run: `python -m pytest scripts/test_build_pa1h_value_universe.py -q`
Expected: FAIL（`resample_1h`/`merge_frames` 未定义）

- [ ] **Step 3: 实现 `scripts/build_pa1h_value_universe.py`**

```python
#!/usr/bin/env python3
"""S2 用：k15m 重采样 1h → 复用 pa_features 算 PA(滞后1日) → merge 财务(as-of) → universe_pa1h_value.csv。"""
import os, glob, sys, csv
import pandas as pd

try:
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:
    pass
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from build_pa_features import pa_features, COLS as PA_COLS

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BS = os.path.join(REPO, "data", "baostock")
K15M = os.path.join(BS, "k15m")
KDAY = os.path.join(BS, "kday")
FUND = os.path.join(REPO, "data", "fundamentals")
OUT = os.path.join(BS, "pa1h_value_merged")
UNIV = os.path.join(BS, "universe_pa1h_value.csv")
FIN_COLS = ["roe", "np_yoy", "rev_yoy", "gross_margin", "eps", "bps"]


def resample_1h(df15):
    """每交易日按顺序每 4 根 15m 合 1 根 1h（open首/high max/low min/close末/volume和）。"""
    d = df15.copy()
    d["time"] = pd.to_datetime(d["time"])
    d = d.sort_values("time").reset_index(drop=True)
    d["date"] = d["time"].dt.normalize()
    out = []
    for _, g in d.groupby("date", sort=True):
        g = g.reset_index(drop=True)
        for i in range(0, len(g), 4):
            blk = g.iloc[i:i + 4]
            out.append({"time": blk["time"].iloc[-1], "open": blk["open"].iloc[0],
                        "high": blk["high"].max(), "low": blk["low"].min(),
                        "close": blk["close"].iloc[-1], "volume": blk["volume"].sum()})
    return pd.DataFrame(out, columns=["time", "open", "high", "low", "close", "volume"])


def merge_frames(pa, fin):
    """pa: time + pa_*(已滞后)；fin: time + 财务(公告日)。→ pa 为基准 as-of-backward 并财务。"""
    fin_cols = [c for c in FIN_COLS if c in fin.columns]
    return pd.merge_asof(pa.sort_values("time"), fin[["time"] + fin_cols].sort_values("time"),
                         on="time", direction="backward")


def merge_one(sym):
    kp = os.path.join(K15M, f"{sym}.csv"); fp = os.path.join(FUND, f"{sym}.csv")
    if not (os.path.exists(kp) and os.path.exists(fp) and os.path.exists(os.path.join(KDAY, f"{sym}.csv"))):
        return False
    os.makedirs(OUT, exist_ok=True)
    h = resample_1h(pd.read_csv(kp, usecols=["time", "open", "high", "low", "close", "volume"]))
    if len(h) < 60:
        return False
    feat = pa_features(h)                       # time + pa_*
    feat[PA_COLS] = feat[PA_COLS].shift(1)      # 滞后1根(无前视)
    feat = feat.iloc[1:].copy()
    feat["time"] = pd.to_datetime(feat["time"]).dt.normalize()  # 1h 戳 → 当日(date) 供日频 as-of
    feat = feat.groupby("time").tail(1)        # 每日最后一根 1h 的 PA = 当日 EOD 1h-PA
    fin = pd.read_csv(fp); fin["time"] = pd.to_datetime(fin["time"])
    m = merge_frames(feat, fin)
    m["time"] = pd.to_datetime(m["time"]).dt.strftime("%Y-%m-%d")
    m.to_csv(os.path.join(OUT, f"{sym}.csv"), index=False)
    return True


def main():
    os.makedirs(OUT, exist_ok=True)
    syms = sorted(os.path.basename(p)[:-4] for p in glob.glob(os.path.join(K15M, "*.csv")))
    ok = []
    for i, s in enumerate(syms, 1):
        try:
            if merge_one(s):
                ok.append(s)
        except Exception as e:
            print(f"  skip {s}: {e}")
        if i % 300 == 0:
            print(f"  {i}/{len(syms)}...")
    with open(UNIV, "w", newline="", encoding="utf-8") as f:
        w = csv.writer(f); w.writerow(["symbol", "primary", "context", "fundamentals"])
        for s in ok:
            w.writerow([s, os.path.join(KDAY, f"{s}.csv").replace("\\", "/"), "",
                        os.path.join(OUT, f"{s}.csv").replace("\\", "/")])
    print(f"wrote {UNIV}: {len(ok)} syms")


if __name__ == "__main__":
    main()
```

注：`build_pa_features.py` 顶部已有 `COLS` 列表（11 个 pa_*）；本脚本 `from build_pa_features import pa_features, COLS as PA_COLS` 复用。1h-PA 每日取最后一根 1h 的 PA 作"当日 EOD 1h 形态"，戳到 date、再滞后已在 `feat[PA_COLS].shift(1)` 完成（按 1h 序列滞后1根=上一根1h；EOD 日频用途下等效昨日尾盘形态）。

- [ ] **Step 4: 跑测试确认通过 + 加 pa1hv 轴**

Run: `python -m pytest scripts/test_build_pa1h_value_universe.py -q` → PASS（2 passed）

`scripts/iterate.py` 的 `AXES` dict 末尾（`paov` 之后、`}` 之前）加：
```python
    "pa1hv": {"universe": "data/baostock/universe_pa1h_value.csv",
              "frm": "2021-01-01", "to": "2026-06-12", "warmup": 60, "window": 60,
              "regimes_hint": "train 2021..2023 / OOS 2024..2026 (value + 1h-PA, S2)"},
```
（1h-PA 源自 k15m=2021 起，故 frm 2021。）ast 校验：`python -c "import ast; ast.parse(open('scripts/iterate.py',encoding='utf-8').read())"`。

- [ ] **Step 5: 提交**

```bash
git add scripts/build_pa1h_value_universe.py scripts/test_build_pa1h_value_universe.py scripts/iterate.py
git commit -F - <<'EOF'
feat(s2): 1h-PA value universe builder (15m->1h resample, reuse pa_features, lag-1) + pa1hv axis
EOF
```

---

### Task 2: 三策略 树 + 配置

**Files:**
- Create: `examples/screen/iter/s1_value_top3.yaml`
- Create: `examples/trees/screen/pa1h_overlay.yaml` + `examples/screen/iter/s2_value_pa1h_top3.yaml`
- Create: `examples/trees/screen/sector_strength.yaml` + `examples/screen/iter/s3_sector_value_top3.yaml`
- Test: `scripts/test_top3_strategy_configs.py`

**Interfaces:**
- Consumes: 价值树（已存在）；S2 universe 的 `fund.pa_*`(Task1)；S3 universe_pa_sector 的 `fund.sec_mom20`。
- Produces: 3 配置供 Task3 跑。

- [ ] **Step 1: 写 S1 配置** `examples/screen/iter/s1_value_top3.yaml`

```yaml
# 策略1：纯价值三核 → top-3。universe=universe_baostock_day，月频 reb20。
quality_trees:
  - examples/trees/screen/value_pb.yaml
  - examples/trees/screen/growth_revyoy.yaml
  - examples/trees/screen/quality_gm.yaml
setup_trees:
  inert: [examples/trees/screen/momentum_xs.yaml]
merge: { q_floor: 0.0, top: 3, lambda: 0.0, tilt_setups: ["inert"], quality_layers: 5 }
regimes:
  - { label: "train", from: 2018-01-02, to: 2023-12-29 }
  - { label: "2024-26_OOS", from: 2024-01-02, to: 2026-06-12 }
```

- [ ] **Step 2: 写 S2 树 + 配置**

`examples/trees/screen/pa1h_overlay.yaml`（纯 PA，无板块项；列名 pa_*，源自 1h）：
```yaml
# S2 setup：1h-PA 入场分（回调+H1H2+窄通道+顺势，仅上升趋势）。无板块项(pa1h universe 无 sec_*)。
meta: { name: pa1h_overlay, forward_window: 5, stances: [long, flat] }
params: { w_pull: 1.0, w_h12: 1.0, w_narrow: 0.5, w_sigw: 0.5, w_struct: 0.5, w_ext: 0.5, w_sigc: 0.5 }
root: gate
nodes:
  gate:
    type: quant
    branches:
      - { when: "fund.pa_dir > 0 or fund.pa_struct >= 1", goto: score, label: uptrend }
    default: { goto: flat, label: not_up }
leaves:
  score:
    stance: long
    weight: "sigmoid( w_pull*fund.pa_pullback*5 + w_h12*(fund.pa_h1 + fund.pa_h2) + w_narrow*(0.05 - fund.pa_chan)*10 + w_sigw*fund.pa_sig_with + w_struct*fund.pa_struct*0.5 - w_ext*fund.pa_ext - w_sigc*fund.pa_sig_cnt )"
  flat: { stance: flat }
```
`examples/screen/iter/s2_value_pa1h_top3.yaml`：
```yaml
# 策略2：价值最便宜~30(value_frac) → 1h-PA 强化排序 → top-3。universe=universe_pa1h_value，周频 reb5。
quality_trees:
  - examples/trees/screen/value_pb.yaml
  - examples/trees/screen/growth_revyoy.yaml
  - examples/trees/screen/quality_gm.yaml
setup_trees:
  pa: [examples/trees/screen/pa1h_overlay.yaml]
merge: { q_floor: 0.0, top: 3, lambda: 1.5, tilt_setups: ["pa"], quality_layers: 5 }
value_frac: 0.03
regimes:
  - { label: "train_21_23", from: 2021-01-04, to: 2023-12-29 }
  - { label: "OOS_24_26", from: 2024-01-02, to: 2026-06-12 }
```

- [ ] **Step 3: 写 S3 树 + 配置**

`examples/trees/screen/sector_strength.yaml`：
```yaml
# S3 setup：板块强度（sec_mom20 越高分越高），深度加权价值。
meta: { name: sector_strength, forward_window: 20, stances: [long, flat] }
params: { sec_scale: 0.1 }
root: gate
nodes:
  gate:
    type: quant
    branches:
      - { when: "fund.bps > 0", goto: s, label: ok }
    default: { goto: flat, label: na }
leaves:
  s: { stance: long, weight: "sigmoid(fund.sec_mom20 / sec_scale)" }
  flat: { stance: flat }
```
`examples/screen/iter/s3_sector_value_top3.yaml`：
```yaml
# 策略3：价值×板块强度 深度加权 → top-3。universe=universe_pa_sector，月频 reb20。
quality_trees:
  - examples/trees/screen/value_pb.yaml
  - examples/trees/screen/growth_revyoy.yaml
  - examples/trees/screen/quality_gm.yaml
setup_trees:
  sec: [examples/trees/screen/sector_strength.yaml]
merge: { q_floor: 0.0, top: 3, lambda: 1.5, tilt_setups: ["sec"], quality_layers: 5 }
regimes:
  - { label: "train", from: 2018-01-02, to: 2023-12-29 }
  - { label: "2024-26_OOS", from: 2024-01-02, to: 2026-06-12 }
```

- [ ] **Step 4: 写加载测试 + 跑** `scripts/test_top3_strategy_configs.py`

```python
#!/usr/bin/env python3
"""3 策略配置 + 2 新树 YAML 合法 + 关键字段。
跑：python -m pytest scripts/test_top3_strategy_configs.py -q"""
import os, yaml
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def _load(rel):
    with open(os.path.join(REPO, rel), encoding="utf-8") as f:
        return yaml.safe_load(f)


def test_three_configs_top3():
    for name in ["s1_value_top3", "s2_value_pa1h_top3", "s3_sector_value_top3"]:
        c = _load(f"examples/screen/iter/{name}.yaml")
        assert c["merge"]["top"] == 3
        assert len(c["quality_trees"]) == 3       # 三核
    s2 = _load("examples/screen/iter/s2_value_pa1h_top3.yaml")
    assert s2["value_frac"] == 0.03 and s2["merge"]["lambda"] == 1.5
    assert "pa" in s2["merge"]["tilt_setups"]
    s3 = _load("examples/screen/iter/s3_sector_value_top3.yaml")
    assert "sec" in s3["merge"]["tilt_setups"]


def test_two_new_trees():
    assert _load("examples/trees/screen/pa1h_overlay.yaml")["meta"]["name"] == "pa1h_overlay"
    assert _load("examples/trees/screen/sector_strength.yaml")["meta"]["name"] == "sector_strength"
```

Run: `python -m pytest scripts/test_top3_strategy_configs.py -q` → PASS（2 passed）
（不跑 cargo 引擎冒烟：S2 universe 在 Task3 才产出；引擎在 Task3 首跑时校验树。）

- [ ] **Step 5: 提交**

```bash
git add examples/screen/iter/s1_value_top3.yaml examples/trees/screen/pa1h_overlay.yaml examples/screen/iter/s2_value_pa1h_top3.yaml examples/trees/screen/sector_strength.yaml examples/screen/iter/s3_sector_value_top3.yaml scripts/test_top3_strategy_configs.py
git commit -F - <<'EOF'
feat(top3): S1/S2/S3 strategy configs + pa1h_overlay & sector_strength trees
EOF
```

---

### Task 3: 验证（3 策略 × {top3, top10}）+ findings

**Files:**
- Create: `docs/superpowers/2026-06-21-top3-concentrated-strategies-findings.md`
- Append: `docs/superpowers/iteration-ledger.md`（iterate.py 自动）
- Run only。

- [ ] **Step 1: 产出 S2 数据**

```bash
python scripts/build_pa1h_value_universe.py
```
Expected: 打印 `wrote ...universe_pa1h_value.csv: <N> syms`（N ≈ 1000+，有 k15m+kday+财务的股）。

- [ ] **Step 2: 跑 3 策略 × top3（vs csi300，主口径）**

```bash
python scripts/iterate.py examples/screen/iter/s1_value_top3.yaml --note "S1 value top3 vs csi300" --axis daily   --rebalance 20 --benchmark csi300 --top 3
python scripts/iterate.py examples/screen/iter/s2_value_pa1h_top3.yaml --note "S2 value+1hPA top3 vs csi300" --axis pa1hv --rebalance 5  --benchmark csi300 --top 3
python scripts/iterate.py examples/screen/iter/s3_sector_value_top3.yaml --note "S3 value x sector top3 vs csi300" --axis paov --rebalance 20 --benchmark csi300 --top 3
```
（S1 用 daily 轴=universe_baostock_day；S3 用 paov 轴=universe_pa_sector，二者均含财务+S3 所需 sec_mom20。）

- [ ] **Step 3: 跑 3 策略 × top10（稳定性参照）+ EW 参考**

对 3 个配置各再跑一遍 `--top 10`（同上命令改 `--top 10`）。再对每个 `--top 3` 各跑一遍**不带 --benchmark**（=vs EW 参考）。记录每轮 net / net-OOS / Sharpe / maxDD / 换手 / verdict。

- [ ] **Step 4: 写 findings + 提交**

`docs/superpowers/2026-06-21-top3-concentrated-strategies-findings.md`：方法学（3 策略机制 + top3/top10 + 双基准）+ 结果表（3×{top3,top10}×{csi300,EW} 的 net/OOS/Sharpe/maxDD/换手/verdict）+ **与价值 top-50 基线对照**（已知 vs csi300 PASS net-OOS+0.28）+ **终判**（哪个策略最优；top-3 噪声 vs top-10 信号；S2/S3 是否增益于 S1；诚实，证伪写清）+ 边界（top-3 噪声/单一OOS/T+1/幸存者）。

```bash
git add docs/superpowers/2026-06-21-top3-concentrated-strategies-findings.md docs/superpowers/iteration-ledger.md
git commit -F - <<'EOF'
research(top3): three concentrated strategies validation — S1/S2/S3 x {top3,top10}, comparison + verdict
EOF
```

---

## Self-Review

**Spec coverage**：S1→T2(s1 config)；S2(价值→1h-PA→top3)→T1(1h builder/pa1hv)+T2(pa1h_overlay+s2 value_frac/lambda)；S3(价值×板块)→T2(sector_strength+s3)；top3+top10 双口径→T3 Step2-3；vs csi300+EW+§5.3→T3；零引擎改动/无前视/1h 重采样→Global+T1。覆盖完整。

**Placeholder scan**：无 TBD；每步完整代码/命令。findings 步明确产出内容。

**Type consistency**：`resample_1h`/`merge_frames`/`merge_one` 跨步一致；S2 复用 `pa_features`+`COLS`（master 已有）；列名 pa_*（11）/sec_mom20/财务（6）一致；universe 路径 `universe_pa1h_value.csv` 在 T1/T3、`pa1hv` 轴在 T1/T3 一致；三核树路径与现有一致。
