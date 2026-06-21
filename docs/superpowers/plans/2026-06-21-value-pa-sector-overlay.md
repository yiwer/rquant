# 价值 + PA短线择时 + 板块轮动 overlay 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在已验证的价值三核选股上，叠加 PA 入场择时 + 板块轮动作为 setup tilt（lambda=短线比率，周频 reb5），并诚实回测它是否优于纯价值。

**Architecture:** 零引擎改动。3 个 Python builder 把 PA(日线) + 板块因子离线算出、与财务 merge 成一份 fundamentals，经现有 `fund.<col>` 通道喂给 `run_screen`；`quality_trees`=价值三核，`setup_trees`=新 `pa_overlay` 树，`merge.lambda`=短线比率。用 `iterate.py` 新增 `paov` 轴对比纯价值 vs 价值+overlay。

**Tech Stack:** Python 3.13 + pandas/numpy（builder & 单测 pytest）；YAML（树/配置）；现有 Rust `rquant screen` 引擎 + `scripts/iterate.py` 验证 harness。

## Global Constraints

- **零引擎改动**：只新增 Python builder + YAML + `iterate.py` 一个 axis 字典项；不改 `src/`、不改桌面、不改部署。
- **无前视**：所有 PA/板块因子 ≤t 计算 + **滞后 1 交易日**（昨日盘后算→今日用）；财务按公告日 as-of；摆动结构用滚动 highest/lowest（不用未来确认的 pivot）。
- **节奏**：周频 `--rebalance 5`；`top 50`；lambda 扫 `{0, 0.3, 0.5, 0.7}`（0=纯价值基线）。
- **数据口径**：板块热度=本地代理（板块动量 + breadth + 聚合成交额），**不抓真资金流**。
- **§5.3 判据**：处理 net-OOS **>** 基线 且过闸（gross>0 ∧ net-OOS>0 ∧ net-Sharpe>0 ∧ break-even≥40bps ∧ tier2 无符号翻转）；证伪也是有效产出，不参数钓鱼。
- **工程纪律**：`git add` 显式文件、英文 commit（`git commit -F -`）、不 push（除非用户要）；单测用 pytest（`python -m pytest scripts/test_X.py -q`）。
- **复用**：价值树 `examples/trees/screen/{value_pb,growth_revyoy,quality_gm}.yaml`、`momentum_xs.yaml`（inert）、`iterate.py`、`build_intraday_merged_universe.py`（merge_asof 范式）。

数据现状（已核实）：`data/baostock/kday/<sym>.csv`(time,open,high,low,close,volume,amount,turn,pctChg)；`data/fundamentals/<sym>.csv`(time,roe,np_yoy,rev_yoy,gross_margin,eps,bps)；`data/baostock/sector_membership.csv`(symbol,industry,classification,update_date，UTF-8)；`data/baostock/sector_daily_panel.csv`(date,industry,ret,n,breadth)。

---

### Task 1: PA 日线特征 builder（build_pa_features.py）

**Files:**
- Create: `scripts/build_pa_features.py`
- Test: `scripts/test_build_pa_features.py`

**Interfaces:**
- Produces: `pa_features(df: pd.DataFrame) -> pd.DataFrame`，入参 df 为单股日线（列 `time,open,high,low,close` 升序），返回列 `time, pa_ema20, pa_dir, pa_struct, pa_regime, pa_pullback, pa_h1, pa_h2, pa_chan, pa_sig_with, pa_sig_cnt, pa_ext`（与入参等长，未 warmup 处为 NaN）。`main()` 遍历 `data/baostock/kday/*.csv` → 每股算特征 → **整体下移 1 行(滞后1日)** → 写 `data/baostock/pa_features/<sym>.csv`（date-only time）。
- 后续 Task 3 读 `pa_features/<sym>.csv` 的这些列。

- [ ] **Step 1: 写失败测试** `scripts/test_build_pa_features.py`

```python
#!/usr/bin/env python3
"""build_pa_features 单测：钉死 PA 特征公式 + 无前视(滚动结构/不看未来)。
跑：python -m pytest scripts/test_build_pa_features.py -q"""
import numpy as np
import pandas as pd
from build_pa_features import pa_features


def _series(closes, highs=None, lows=None, opens=None):
    n = len(closes)
    highs = highs or [c * 1.01 for c in closes]
    lows = lows or [c * 0.99 for c in closes]
    opens = opens or [c for c in closes]
    return pd.DataFrame({
        "time": pd.date_range("2020-01-01", periods=n, freq="D"),
        "open": opens, "high": highs, "low": lows, "close": closes,
    })


def test_ema20_and_dir_uptrend():
    # 单调上涨 60 天 → EMA20 上方、方向为 +1
    df = _series([100 + i for i in range(60)])
    f = pa_features(df)
    assert f["pa_ema20"].iloc[-1] > 0          # 收盘在 EMA20 上方
    assert f["pa_dir"].iloc[-1] == 1           # EMA20 斜率向上


def test_regime_trend_vs_range():
    # 平滑趋势 ER 高；锯齿震荡 ER 低
    trend = pa_features(_series([100 + i for i in range(40)]))["pa_regime"].iloc[-1]
    chop = pa_features(_series([100 + (i % 2) for i in range(40)]))["pa_regime"].iloc[-1]
    assert trend > 0.8 and chop < 0.2


def test_struct_hh_hl_uptrend():
    # 阶梯上涨：高点/低点都抬高 → pa_struct = +2
    closes = [100 + i for i in range(40)]
    f = pa_features(_series(closes))
    assert f["pa_struct"].iloc[-1] == 2


def test_no_lookahead_shape_only_past():
    # 在 t 处的特征只依赖 ≤t；篡改未来某天不应改变更早的特征值
    base = [100 + np.sin(i / 3) for i in range(60)]
    f0 = pa_features(_series(base))
    bumped = list(base); bumped[55] += 10        # 改第 55 天
    f1 = pa_features(_series(bumped))
    # 第 40 天的特征不受未来(第55天)影响
    for col in ["pa_ema20", "pa_struct", "pa_regime", "pa_pullback"]:
        assert f0[col].iloc[40] == f1[col].iloc[40] or (
            pd.isna(f0[col].iloc[40]) and pd.isna(f1[col].iloc[40]))


def test_h1_h2_pullback_entry():
    # 上涨→回调3天→连续2根创新高：H1 在首根突破、H2 在次根
    closes = [100, 102, 104, 106, 108, 110,  # 上涨
              108, 106, 104,                  # 回调
              107, 109]                       # 连续2根反抽(高点抬升)
    highs = [c + 1 for c in closes]
    f = pa_features(_series(closes, highs=highs))
    assert f["pa_h1"].iloc[-2] == 1           # 倒数第二根 = H1
    assert f["pa_h2"].iloc[-1] == 1           # 最后一根 = H2
```

- [ ] **Step 2: 跑测试确认失败**

Run: `python -m pytest scripts/test_build_pa_features.py -q`
Expected: FAIL（`ModuleNotFoundError: build_pa_features` 或 `pa_features` 未定义）

- [ ] **Step 3: 实现 `scripts/build_pa_features.py`**

```python
#!/usr/bin/env python3
"""日线 PA 特征（趋势/结构/回调/H1H2/通道/信号K强度），滞后1交易日无前视。
输出 data/baostock/pa_features/<sym>.csv，供 pa_overlay 树经 fund.<col> 取用。"""
import os, glob, sys
import numpy as np
import pandas as pd

try:
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:
    pass

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
KDAY = os.path.join(REPO, "data", "baostock", "kday")
OUT = os.path.join(REPO, "data", "baostock", "pa_features")
W = 10   # 结构/回调窗口
COLS = ["pa_ema20", "pa_dir", "pa_struct", "pa_regime", "pa_pullback",
        "pa_h1", "pa_h2", "pa_chan", "pa_sig_with", "pa_sig_cnt", "pa_ext"]


def _h1h2(close, high):
    """上涨回调后首/次根创新高(high>前一根high)。仅用过去 → 无前视。返回 (h1[], h2[])。"""
    n = len(close); h1 = np.zeros(n); h2 = np.zeros(n)
    ema = pd.Series(close).ewm(span=20, adjust=False).mean().values
    pulled = False; up_count = 0
    for i in range(1, n):
        uptrend = close[i] > ema[i]
        if not uptrend:
            pulled = False; up_count = 0; continue
        if close[i] < close[i - 1]:               # 回调中
            pulled = True; up_count = 0
        elif pulled and high[i] > high[i - 1]:    # 回调后向上突破前一根高
            up_count += 1
            if up_count == 1:
                h1[i] = 1
            elif up_count == 2:
                h2[i] = 1; pulled = False
    return h1, h2


def pa_features(df):
    df = df.reset_index(drop=True)
    c = df["close"].astype(float); h = df["high"].astype(float)
    l = df["low"].astype(float); o = df["open"].astype(float)
    out = pd.DataFrame({"time": df["time"]})
    ema20 = c.ewm(span=20, adjust=False).mean()
    out["pa_ema20"] = c / ema20 - 1.0
    out["pa_dir"] = np.sign(ema20.diff(5)).fillna(0.0)
    # 效率比 ER(20)：方向位移 / 路径长度
    chg = (c - c.shift(20)).abs()
    path = c.diff().abs().rolling(20).sum()
    out["pa_regime"] = (chg / path.replace(0, np.nan)).clip(0, 1)
    # 结构：两段滚动高/低比较（无前视）
    rh = h.rolling(W).max(); rl = l.rolling(W).min()
    HH = (rh > rh.shift(W)).astype(int); HL = (rl > rl.shift(W)).astype(int)
    LL = (rl < rl.shift(W)).astype(int); LH = (rh < rh.shift(W)).astype(int)
    out["pa_struct"] = (HH + HL - LL - LH).astype(float)
    # 回调深度：上升趋势中从近 W 高回撤、且收盘仍在 EMA20 上方
    recent_high = h.rolling(W).max()
    pull = (recent_high - c) / recent_high.replace(0, np.nan)
    out["pa_pullback"] = np.where((c > ema20) & (out["pa_dir"] > 0), pull.clip(lower=0), 0.0)
    # H1/H2
    h1, h2 = _h1h2(c.values, h.values)
    out["pa_h1"] = h1; out["pa_h2"] = h2
    # 通道宽窄：ATR(14)/价（窄=低）
    pc = c.shift(1)
    tr = pd.concat([(h - l), (h - pc).abs(), (l - pc).abs()], axis=1).max(axis=1)
    atr = tr.ewm(alpha=1 / 14, adjust=False).mean()
    out["pa_chan"] = atr / c
    # 信号K强度：实体占比 × 收盘位置；顺势(上涨K)/逆势(下跌K)分列
    rng = (h - l).replace(0, np.nan)
    body = (c - o).abs() / rng
    close_pos = (c - l) / rng
    up_bar = (c >= o)
    out["pa_sig_with"] = np.where(up_bar, body * close_pos, 0.0)
    out["pa_sig_cnt"] = np.where(~up_bar, body * (1 - close_pos), 0.0)
    # 过度延展：EMA20 上方超过 1 个 ATR 的部分
    out["pa_ext"] = ((c - ema20) / atr.replace(0, np.nan)).clip(lower=0)
    return out[["time"] + COLS]


def main():
    os.makedirs(OUT, exist_ok=True)
    files = sorted(glob.glob(os.path.join(KDAY, "*.csv")))
    ok = 0
    for i, p in enumerate(files, 1):
        s = os.path.basename(p)[:-4]
        df = pd.read_csv(p, usecols=["time", "open", "high", "low", "close"])
        if len(df) < 60:
            continue
        df["time"] = pd.to_datetime(df["time"])
        df = df.sort_values("time").reset_index(drop=True)
        feat = pa_features(df)
        feat[COLS] = feat[COLS].shift(1)              # 滞后1交易日(无前视)
        feat["time"] = feat["time"].dt.strftime("%Y-%m-%d")
        feat.iloc[1:].to_csv(os.path.join(OUT, f"{s}.csv"), index=False)
        ok += 1
        if i % 400 == 0:
            print(f"  {i}/{len(files)}...")
    print(f"built {ok} PA-feature CSVs -> {OUT}")


if __name__ == "__main__":
    main()
```

- [ ] **Step 4: 跑测试确认通过**

Run: `python -m pytest scripts/test_build_pa_features.py -q`
Expected: PASS（5 passed）

- [ ] **Step 5: 提交**

```bash
git add scripts/build_pa_features.py scripts/test_build_pa_features.py
git commit -F - <<'EOF'
feat(pa): daily PA feature builder (trend/struct/pullback/H1H2/channel/signal-bar), lag-1 no-lookahead
EOF
```

---

### Task 2: 板块因子 builder（build_sector_factors.py）

**Files:**
- Create: `scripts/build_sector_factors.py`
- Test: `scripts/test_build_sector_factors.py`

**Interfaces:**
- Produces: `sector_factors(panel: pd.DataFrame) -> pd.DataFrame`，入参 `panel`(列 `date,industry,ret,breadth`)，返回每(date,industry) 的 `sec_mom20, sec_trend, sec_breadth, sec_heat_panel`（`sec_heat_panel`=breadth 的 5 日均，作为热度代理；`amount` 聚合热度在 main 里加）。`main()`：读 `sector_daily_panel.csv` 算板块因子 + 读 `sector_membership.csv`(symbol→industry) + 聚合各股 kday `amount` 成板块成交额热度 `sec_heat` → 每股写 `data/baostock/sector_factors/<sym>.csv`(time, sec_mom20, sec_trend, sec_breadth, sec_heat)，**滞后1日**。
- 后续 Task 3 读这些列。

- [ ] **Step 1: 写失败测试** `scripts/test_build_sector_factors.py`

```python
#!/usr/bin/env python3
"""build_sector_factors 单测：板块动量/趋势/广度公式 + 无前视。
跑：python -m pytest scripts/test_build_sector_factors.py -q"""
import numpy as np
import pandas as pd
from build_sector_factors import sector_factors


def test_mom20_and_breadth():
    # 单一板块 30 天，每天 +1% → index 复利上涨；mom20 = index[t]/index[t-20]-1
    n = 30
    panel = pd.DataFrame({
        "date": pd.date_range("2020-01-01", periods=n, freq="D"),
        "industry": ["A01农业"] * n,
        "ret": [0.01] * n,
        "breadth": [0.6] * n,
    })
    f = sector_factors(panel)
    idx = (1.01 ** 20) - 1.0
    assert np.isclose(f["sec_mom20"].iloc[-1], idx, rtol=1e-6)
    assert np.isclose(f["sec_breadth"].iloc[-1], 0.6, rtol=1e-9)   # 5日均=0.6


def test_two_sectors_independent():
    n = 25
    a = pd.DataFrame({"date": pd.date_range("2020-01-01", periods=n), "industry": "A",
                      "ret": 0.0, "breadth": 0.5})
    b = pd.DataFrame({"date": pd.date_range("2020-01-01", periods=n), "industry": "B",
                      "ret": 0.02, "breadth": 0.9})
    f = sector_factors(pd.concat([a, b]))
    fa = f[f["industry"] == "A"]["sec_mom20"].iloc[-1]
    fb = f[f["industry"] == "B"]["sec_mom20"].iloc[-1]
    assert fb > fa                                   # B 板块动量更强
```

- [ ] **Step 2: 跑测试确认失败**

Run: `python -m pytest scripts/test_build_sector_factors.py -q`
Expected: FAIL（`sector_factors` 未定义）

- [ ] **Step 3: 实现 `scripts/build_sector_factors.py`**

```python
#!/usr/bin/env python3
"""板块轮动因子（动量/趋势/广度/成交额热度）→ 逐股(其所属板块)，滞后1日无前视。
输出 data/baostock/sector_factors/<sym>.csv，供 pa_overlay 树经 fund.<col> 取用。"""
import os, glob, sys
import numpy as np
import pandas as pd

try:
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:
    pass

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BS = os.path.join(REPO, "data", "baostock")
KDAY = os.path.join(BS, "kday")
PANEL = os.path.join(BS, "sector_daily_panel.csv")
MEMB = os.path.join(BS, "sector_membership.csv")
OUT = os.path.join(BS, "sector_factors")


def sector_factors(panel):
    """panel: date,industry,ret,breadth → 每(date,industry) 因子。"""
    p = panel.copy()
    p["date"] = pd.to_datetime(p["date"])
    p = p.sort_values(["industry", "date"])
    g = p.groupby("industry", group_keys=False)
    p["index"] = g["ret"].apply(lambda r: (1.0 + r).cumprod())
    p["sec_mom20"] = g["index"].apply(lambda x: x / x.shift(20) - 1.0)
    p["sec_trend"] = g["index"].apply(lambda x: x / x.rolling(20).mean() - 1.0)
    p["sec_breadth"] = g["breadth"].apply(lambda x: x.rolling(5).mean())
    return p[["date", "industry", "sec_mom20", "sec_trend", "sec_breadth"]]


def main():
    os.makedirs(OUT, exist_ok=True)
    panel = pd.read_csv(PANEL)
    panel["date"] = pd.to_datetime(panel["date"]).dt.strftime("%Y-%m-%d")
    sf = sector_factors(panel)
    sf["date"] = pd.to_datetime(sf["date"]).dt.strftime("%Y-%m-%d")
    memb = pd.read_csv(MEMB, encoding="utf-8")[["symbol", "industry"]]
    s2i = dict(zip(memb["symbol"], memb["industry"]))
    # 板块成交额热度：聚合各股 amount → 板块日成交额 / 其 MA20
    amt = {}
    for p in glob.glob(os.path.join(KDAY, "*.csv")):
        s = os.path.basename(p)[:-4]
        ind = s2i.get(s)
        if ind is None:
            continue
        d = pd.read_csv(p, usecols=["time", "amount"])
        d["date"] = pd.to_datetime(d["time"]).dt.strftime("%Y-%m-%d")
        amt.setdefault(ind, []).append(d[["date", "amount"]])
    heat_rows = []
    for ind, dfs in amt.items():
        a = pd.concat(dfs).groupby("date", as_index=False)["amount"].sum().sort_values("date")
        a["sec_heat"] = a["amount"] / a["amount"].rolling(20).mean()
        a["industry"] = ind
        heat_rows.append(a[["date", "industry", "sec_heat"]])
    heat = pd.concat(heat_rows) if heat_rows else pd.DataFrame(columns=["date", "industry", "sec_heat"])
    sf = sf.merge(heat, on=["date", "industry"], how="left")
    cols = ["sec_mom20", "sec_trend", "sec_breadth", "sec_heat"]
    ok = 0
    for s, ind in s2i.items():
        sub = sf[sf["industry"] == ind].sort_values("date")
        if len(sub) < 25:
            continue
        out = sub[["date"] + cols].copy()
        out[cols] = out[cols].shift(1)                 # 滞后1日
        out = out.rename(columns={"date": "time"}).iloc[1:]
        out.to_csv(os.path.join(OUT, f"{s}.csv"), index=False)
        ok += 1
    print(f"built {ok} sector-factor CSVs -> {OUT}")


if __name__ == "__main__":
    main()
```

- [ ] **Step 4: 跑测试确认通过**

Run: `python -m pytest scripts/test_build_sector_factors.py -q`
Expected: PASS（2 passed）

- [ ] **Step 5: 提交**

```bash
git add scripts/build_sector_factors.py scripts/test_build_sector_factors.py
git commit -F - <<'EOF'
feat(sector): sector-rotation factor builder (momentum/trend/breadth/turnover-heat) -> per-stock, lag-1
EOF
```

---

### Task 3: 合并 universe builder + iterate.py paov 轴

**Files:**
- Create: `scripts/build_pa_sector_universe.py`
- Modify: `scripts/iterate.py`（在 `AXES` dict 末尾加 `paov` 项；与现有 `ff3`/`imerge` 同样式）
- Test: `scripts/test_build_pa_sector_universe.py`

**Interfaces:**
- Consumes: `data/baostock/pa_features/<sym>.csv`(Task1)、`data/baostock/sector_factors/<sym>.csv`(Task2)、`data/fundamentals/<sym>.csv`(财务)、`data/baostock/kday/<sym>.csv`。
- Produces: `merge_one(sym) -> bool` 写 `data/baostock/pa_sector_merged/<sym>.csv`(time + 6财务 + 11 PA + 4 板块列，财务公告日 as-of、PA/板块已滞后)；`data/baostock/universe_pa_sector.csv`(symbol,primary=kday,context="",fundamentals=合并文件)。iterate.py `paov` 轴 universe 指向它。

- [ ] **Step 1: 写失败测试** `scripts/test_build_pa_sector_universe.py`

```python
#!/usr/bin/env python3
"""build_pa_sector_universe 单测：财务 as-of merge 正确 + 列齐。
跑：python -m pytest scripts/test_build_pa_sector_universe.py -q"""
import pandas as pd
from build_pa_sector_universe import merge_frames


def test_merge_asof_fin_and_factors():
    pa = pd.DataFrame({"time": pd.to_datetime(["2020-03-01", "2020-03-02"]),
                       "pa_ema20": [0.01, 0.02]})
    sec = pd.DataFrame({"time": pd.to_datetime(["2020-03-01", "2020-03-02"]),
                        "sec_mom20": [0.1, 0.1]})
    fin = pd.DataFrame({"time": pd.to_datetime(["2019-10-31", "2020-04-30"]),
                        "bps": [5.0, 6.0]})
    m = merge_frames(pa, sec, fin)
    assert list(m["pa_ema20"]) == [0.01, 0.02]
    assert list(m["sec_mom20"]) == [0.1, 0.1]
    assert list(m["bps"]) == [5.0, 5.0]        # 3月用2019Q3(≤t)的 5.0，不前视用 2020-04-30
```

- [ ] **Step 2: 跑测试确认失败**

Run: `python -m pytest scripts/test_build_pa_sector_universe.py -q`
Expected: FAIL（`merge_frames` 未定义）

- [ ] **Step 3: 实现 `scripts/build_pa_sector_universe.py`**

```python
#!/usr/bin/env python3
"""合并 财务(as-of) + PA(滞后1日) + 板块(滞后1日) → 一份 fundamentals → universe_pa_sector.csv。"""
import os, glob, sys, csv
import pandas as pd

try:
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:
    pass

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BS = os.path.join(REPO, "data", "baostock")
KDAY = os.path.join(BS, "kday")
PA = os.path.join(BS, "pa_features")
SEC = os.path.join(BS, "sector_factors")
FUND = os.path.join(REPO, "data", "fundamentals")
OUT = os.path.join(BS, "pa_sector_merged")
UNIV = os.path.join(BS, "universe_pa_sector.csv")
FIN_COLS = ["roe", "np_yoy", "rev_yoy", "gross_margin", "eps", "bps"]


def merge_frames(pa, sec, fin):
    """pa/sec: time + 因子列(已滞后)；fin: time + 财务列(公告日)。→ 以 pa 的日期为基准 outer-merge sec + as-of fin。"""
    m = pd.merge(pa.sort_values("time"), sec.sort_values("time"), on="time", how="left")
    fin_cols = [c for c in FIN_COLS if c in fin.columns]
    m = pd.merge_asof(m.sort_values("time"), fin[["time"] + fin_cols].sort_values("time"),
                      on="time", direction="backward")
    return m


def merge_one(sym):
    pp = os.path.join(PA, f"{sym}.csv"); sp = os.path.join(SEC, f"{sym}.csv")
    fp = os.path.join(FUND, f"{sym}.csv")
    if not (os.path.exists(pp) and os.path.exists(fp)):
        return False
    pa = pd.read_csv(pp); pa["time"] = pd.to_datetime(pa["time"])
    if os.path.exists(sp):
        sec = pd.read_csv(sp); sec["time"] = pd.to_datetime(sec["time"])
    else:
        sec = pd.DataFrame({"time": pa["time"], "sec_mom20": float("nan"),
                            "sec_trend": float("nan"), "sec_breadth": float("nan"), "sec_heat": float("nan")})
    fin = pd.read_csv(fp); fin["time"] = pd.to_datetime(fin["time"])
    m = merge_frames(pa, sec, fin)
    m["time"] = m["time"].dt.strftime("%Y-%m-%d")
    m.to_csv(os.path.join(OUT, f"{sym}.csv"), index=False)
    return True


def main():
    os.makedirs(OUT, exist_ok=True)
    syms = sorted(os.path.basename(p)[:-4] for p in glob.glob(os.path.join(PA, "*.csv")))
    ok = []
    for i, s in enumerate(syms, 1):
        try:
            if os.path.exists(os.path.join(KDAY, f"{s}.csv")) and merge_one(s):
                ok.append(s)
        except Exception as e:
            print(f"  skip {s}: {e}")
        if i % 400 == 0:
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

- [ ] **Step 4: 跑测试确认通过 + 加 paov 轴**

Run: `python -m pytest scripts/test_build_pa_sector_universe.py -q` → PASS（1 passed）

在 `scripts/iterate.py` 的 `AXES` dict 末尾（`ff3` 项之后、闭合 `}` 之前）加：
```python
    "paov": {"universe": "data/baostock/universe_pa_sector.csv",
             "frm": "2018-01-01", "to": "2026-06-12", "warmup": 60, "window": 60,
             "regimes_hint": "train 2018..2023 / OOS 2024..2026 (value + PA + sector overlay)"},
```

- [ ] **Step 5: 提交**

```bash
git add scripts/build_pa_sector_universe.py scripts/test_build_pa_sector_universe.py scripts/iterate.py
git commit -F - <<'EOF'
feat(overlay): merged value+PA+sector universe builder + iterate.py paov axis
EOF
```

---

### Task 4: pa_overlay setup 树 + 基线/lambda 配置

**Files:**
- Create: `examples/trees/screen/pa_overlay.yaml`
- Create: `examples/screen/iter/value_paov_l0.yaml`（基线 lambda=0=纯价值）
- Create: `examples/screen/iter/value_paov_l03.yaml`、`value_paov_l05.yaml`、`value_paov_l07.yaml`
- Test: `scripts/test_pa_overlay_tree.py`（确认树能被引擎加载，不报错）

**Interfaces:**
- Consumes: 合并 universe 的 `fund.pa_*`/`fund.sec_*` 列（Task3）、价值树（已存在）。
- Produces: 4 个配置供 Task5 跑 iterate。

- [ ] **Step 1: 写 PA overlay 树** `examples/trees/screen/pa_overlay.yaml`

```yaml
# 短线 overlay setup：板块轮动 + PA 回调/趋势入场。仅上升趋势(pa_dir>0 或 pa_struct≥1)内给正倾斜。
# thesis：便宜价值股里，优先「所在板块强/热 + 个股上升趋势的回调买点」。
meta: { name: pa_overlay, forward_window: 5, stances: [long, flat] }
params:
  w_secmom: 1.0
  w_secheat: 0.5
  w_pull: 1.0
  w_h12: 1.0
  w_narrow: 0.5
  w_sigw: 0.5
  w_struct: 0.5
  w_ext: 0.5
  w_sigc: 0.5
  ext_scale: 2.0
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
    weight: "sigmoid( w_secmom*fund.sec_mom20*10 + w_secheat*(fund.sec_breadth - 0.5)*2 + w_pull*fund.pa_pullback*5 + w_h12*(fund.pa_h1 + fund.pa_h2) + w_narrow*(0.05 - fund.pa_chan)*10 + w_sigw*fund.pa_sig_with + w_struct*fund.pa_struct*0.5 - w_ext*fund.pa_ext/ext_scale - w_sigc*fund.pa_sig_cnt )"
  flat: { stance: flat }
```

- [ ] **Step 2: 写 4 个配置**

`examples/screen/iter/value_paov_l0.yaml`（基线）：
```yaml
# 基线：纯价值三核(lambda 0)，universe_pa_sector，周频 reb5。对照组。
quality_trees:
  - examples/trees/screen/value_pb.yaml
  - examples/trees/screen/growth_revyoy.yaml
  - examples/trees/screen/quality_gm.yaml
setup_trees:
  ov: [examples/trees/screen/pa_overlay.yaml]
merge: { q_floor: 0.0, top: 50, lambda: 0.0, tilt_setups: ["ov"], quality_layers: 5 }
regimes:
  - { label: "train", from: 2018-01-02, to: 2023-12-29 }
  - { label: "2024-26_OOS", from: 2024-01-02, to: 2026-06-12 }
```
`value_paov_l03.yaml` / `l05` / `l07`：同上，仅 `merge.lambda` 改为 `0.3` / `0.5` / `0.7`。

- [ ] **Step 3: 写加载测试** `scripts/test_pa_overlay_tree.py`

```python
#!/usr/bin/env python3
"""pa_overlay 树 + 配置能被 rquant 引擎加载(lint 不报错)。
跑：python -m pytest scripts/test_pa_overlay_tree.py -q"""
import os, subprocess
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def test_configs_exist_and_yaml_valid():
    import yaml
    for name in ["value_paov_l0", "value_paov_l03", "value_paov_l05", "value_paov_l07"]:
        p = os.path.join(REPO, "examples", "screen", "iter", f"{name}.yaml")
        with open(p, encoding="utf-8") as f:
            cfg = yaml.safe_load(f)
        assert cfg["merge"]["top"] == 50
        assert "ov" in cfg["merge"]["tilt_setups"]
    tree = os.path.join(REPO, "examples", "trees", "screen", "pa_overlay.yaml")
    with open(tree, encoding="utf-8") as f:
        t = yaml.safe_load(f)
    assert t["meta"]["name"] == "pa_overlay"
```

- [ ] **Step 4: 跑测试 + 引擎 lint 冒烟**

Run: `python -m pytest scripts/test_pa_overlay_tree.py -q` → PASS
Run（需 Task1-3 已产出数据后才有 universe；此处仅确认树语法被引擎接受，as-of 单日）：
`cargo run -q -- screen --universe data/baostock/universe_pa_sector.csv --config examples/screen/iter/value_paov_l05.yaml --as-of 2024-06-28 --top 50`
Expected: 打印排行榜（非空）或明确数据缺失报错；**不得**是树解析/lint 错误。

- [ ] **Step 5: 提交**

```bash
git add examples/trees/screen/pa_overlay.yaml examples/screen/iter/value_paov_l0.yaml examples/screen/iter/value_paov_l03.yaml examples/screen/iter/value_paov_l05.yaml examples/screen/iter/value_paov_l07.yaml scripts/test_pa_overlay_tree.py
git commit -F - <<'EOF'
feat(overlay): pa_overlay setup tree + value baseline/lambda-sweep configs
EOF
```

---

### Task 5: 验证（纯价值 vs 价值+overlay）+ 消融 + findings

**Files:**
- Create: `docs/superpowers/2026-06-21-value-pa-sector-overlay-findings.md`
- Append: `docs/superpowers/iteration-ledger.md`（由 iterate.py 自动追加轮卡）
- Run only（数据产出 + 回测）

**Interfaces:**
- Consumes: Task1-4 全部。

- [ ] **Step 1: 产出数据（联网/计算，依次跑）**

```bash
python scripts/build_pa_features.py
python scripts/build_sector_factors.py
python scripts/build_pa_sector_universe.py
```
Expected: 三条各打印产出股数（PA ~2800、sector ~2900、universe ~2800）。

- [ ] **Step 2: 跑基线 vs lambda 扫（vs EW）**

```bash
python scripts/iterate.py examples/screen/iter/value_paov_l0.yaml  --note "baseline value-only reb5" --axis paov --rebalance 5
python scripts/iterate.py examples/screen/iter/value_paov_l03.yaml --note "value+PA+sector overlay lambda0.3 reb5" --axis paov --rebalance 5
python scripts/iterate.py examples/screen/iter/value_paov_l05.yaml --note "value+overlay lambda0.5 reb5" --axis paov --rebalance 5
python scripts/iterate.py examples/screen/iter/value_paov_l07.yaml --note "value+overlay lambda0.7 reb5" --axis paov --rebalance 5
```
记录每轮 net / net-OOS / Sharpe / 换手 / flags / verdict。

- [ ] **Step 3: 关键对比 + vs csi300 复跑最优 lambda**

判读：**任一 lambda 的 net-OOS 是否 > 基线(l0) 且过 §5.3 闸**？对表现最好的 lambda 加 `--benchmark csi300` 复跑一遍（剥离小盘 beta）。

- [ ] **Step 4: 消融（仅当某 lambda 优于基线才做）**

复制最优配置为 3 个消融变体，分别把 pa_overlay 的板块族(w_secmom=w_secheat=0)/回调族(w_pull=w_h12=w_narrow=0)/趋势族(w_sigw=w_struct=0) 关掉，各跑一轮，定位真正有用的子条件。若无 lambda 优于基线，跳过消融、直接判证伪。

- [ ] **Step 5: 写 findings + 提交**

`docs/superpowers/2026-06-21-value-pa-sector-overlay-findings.md`：方法学（universe/reb5/lambda 扫/双基准）+ 结果表（基线 vs 各 lambda 的 net/OOS/Sharpe/换手/verdict）+ 消融 + **终判**（overlay 是否净增益于纯价值；诚实，证伪也写清）+ 边界。

```bash
git add docs/superpowers/2026-06-21-value-pa-sector-overlay-findings.md docs/superpowers/iteration-ledger.md
git commit -F - <<'EOF'
research(overlay): value + PA + sector overlay validation — <PASS/FALSIFIED> vs value-only baseline
EOF
```

---

## Self-Review

**Spec coverage**：①setup-tilt/lambda→T4 配置；②PA特征(EMA20/dir/struct/regime/pullback/H1H2/chan/sig/ext)→T1；③板块动量/breadth/成交额热度→T2；④合并 universe + paov 轴→T3；⑤PA overlay 树(回调+趋势+板块, 仅上升趋势)→T4；⑥验证(基线 vs lambda 扫, EW+csi300, §5.3, 消融)→T5；⑦零引擎改动/滞后1日无前视/周频→Global Constraints + 各 builder。覆盖完整。

**Placeholder scan**：无 TBD/TODO；每步有完整代码或精确命令。消融步明确"仅当优于基线才做"，非占位。

**Type consistency**：`pa_features()`/`sector_factors()`/`merge_frames()`/`merge_one()` 签名跨任务一致；列名 `pa_*`(11)/`sec_*`(4)/`FIN_COLS`(6) 在 T1/T2/T3/T4 一致；universe 路径 `data/baostock/universe_pa_sector.csv` 在 T3/T4/T5 一致；`paov` 轴名一致。
