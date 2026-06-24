# MVP 选股读路径脊柱 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把现有冻结 ridge 策略经新服务脊柱(注册表 → 因子 as-of → 选股 → API)出 as-of 选股,与 `paper_ridge.py --asof` 逐票/同分一致。

**Architecture:** 方案 A(契约优先模块化单体)。新建 `service/rqs/` Python 包,各服务为模块 + 显式接口;**复用** `scripts/` 下已验证的 `build_factor_matrix`/`eval_ridge._eligible`/`paper_ridge.select_picks`/`norm_gauss`(绞杀者式包装,平价由复用保证)。MVP 只做服务化架构 spec 的流②(选股读路径),不碰训练/采集中心/多租户。

**Tech Stack:** Python 3.13、pydantic 2、FastAPI + uvicorn、pytest 9、pandas/numpy。注册表后端 MVP 用**文件系统 JSON**(对象存储 = `data/registry/`);**Postgres 留到阶段 2**(Registry 接口即替换接缝,见 spec §10)。

## Global Constraints

- Python 3.13、pytest 9(`python -m pytest <file> -q`);测试为纯函数 + 合成数据优先,真数据/网络测试单独标注。
- **复现性第一**:验收 = golden parity(新路径 == `paper_ridge.py --asof` 同日同票)。
- **不污染 qfq 规范数据**:MVP 只读 `data/baostock/kday`,绝不写当日 bar 进 canonical(补数还原是阶段 2 采集中心的事)。
- **冻结权重不可变**:导入的 `paper_ridge_weights.json` 原样成为 frozen 模型,不重训。
- **UTF-8 stdout 纪律**:任何会 print 中文的入口先 `sys.stdout.reconfigure(encoding="utf-8")`(Windows 默认 GBK)。
- **DRY/YAGNI/TDD/频繁提交**:复用 `scripts/` 现有函数,不重实现评分逻辑;每任务一个独立可测交付物。
- commit message 用英文;代码注释/标识用英文。

## 文件结构

```
service/
  rqs/
    __init__.py        # 包标识(空)
    paths.py           # 仓库根派生的路径单一出口
    models.py          # pydantic 契约:StrategyConfig / ModelRecord / Pick / PickResult
    registry.py        # StrategyRegistry + ModelRegistry(文件系统 JSON)
    data.py            # S1 最小:latest_data_date / freshness
    factors.py         # S2:asof_cross_section(读+按需重建 factors_asof.csv)
    selection.py       # S6:asof_pick(复用 select_picks,出 PickResult)
    api.py             # S7:FastAPI,GET /strategies/{sid}/picks
  tests/
    test_registry.py
    test_data.py
    test_factors.py
    test_selection.py
    test_parity.py     # 黄金平价(集成,离线建 as-of)
    test_api.py
  requirements.txt
  Dockerfile
  README.md
```

每个任务结束都是一个独立可测交付物。所有 `service/rqs/*.py` 在导入 `scripts/` 下模块前先 `sys.path.insert(0, paths.SCRIPTS)`。

---

### Task 1: 包脚手架 + 路径出口 + 契约 schema

**Files:**
- Create: `service/rqs/__init__.py`
- Create: `service/rqs/paths.py`
- Create: `service/rqs/models.py`
- Create: `service/tests/test_models_smoke.py`
- Create: `service/requirements.txt`

**Interfaces:**
- Produces: `paths.REPO/SCRIPTS/KDAY/REGISTRY_DIR/ASOF_CSV/ST_PATH/NAMES_PATH`(均为绝对路径 str);`models.StrategyConfig/ModelRecord/Pick/PickResult`(pydantic v2 BaseModel)。

- [ ] **Step 1: 写失败测试**

`service/tests/test_models_smoke.py`:
```python
import os, sys
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from rqs.models import StrategyConfig, ModelRecord, Pick, PickResult
from rqs import paths


def test_strategy_config_defaults():
    cfg = StrategyConfig(strategy_id="ridge-gauss", version=1, factor_subset=["f_bm", "f_mom20"])
    assert cfg.normalization == "gauss"
    assert cfg.selection.top_n == 3
    assert cfg.model.type == "ridge"


def test_model_record_roundtrip():
    m = ModelRecord(model_id="m1", strategy_id="ridge-gauss", strategy_version=1,
                    train_lo="2018-02-06", train_hi="2026-06-04",
                    factor_cols=["f_bm"], factor_cols_hash="abc",
                    weights=[0.1], delta=0.05, top_n=3, ridge_a=0.1, cost_bps=20.0,
                    created="2026-06-24T00:00:00")
    assert ModelRecord.model_validate_json(m.model_dump_json()).model_id == "m1"


def test_pick_result_shape():
    pr = PickResult(strategy_id="s", model_id="m", asof="2026-06-24",
                    picks=[Pick(rank=1, symbol="sz000039", name="中集集团", score=0.1)],
                    eligible_count=876, hysteresis=True)
    assert pr.picks[0].symbol == "sz000039"


def test_paths_under_repo():
    assert paths.SCRIPTS.endswith("scripts")
    assert os.path.isdir(paths.SCRIPTS)
```

- [ ] **Step 2: 跑测试确认失败**

Run: `python -m pytest service/tests/test_models_smoke.py -q`
Expected: FAIL(`ModuleNotFoundError: No module named 'rqs'`)

- [ ] **Step 3: 写实现**

`service/rqs/__init__.py`: (空文件)

`service/rqs/paths.py`:
```python
import os

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SCRIPTS = os.path.join(REPO, "scripts")
KDAY = os.path.join(REPO, "data", "baostock", "kday")
ASOF_CSV = os.path.join(REPO, "data", "factor_panel", "factors_asof.csv")
ST_PATH = os.path.join(REPO, "data", "baostock", "st_symbols.csv")
NAMES_PATH = os.path.join(REPO, "data", "baostock", "stock_names.csv")
REGISTRY_DIR = os.path.join(REPO, "data", "registry")
PAPER_WEIGHTS = os.path.join(REPO, "data", "factor_panel", "paper_ridge_weights.json")
```

`service/rqs/models.py`:
```python
from typing import Literal
from pydantic import BaseModel, Field


class ModelSpec(BaseModel):
    type: Literal["ridge", "nonlinear", "gbdt", "blend"] = "ridge"
    ridge_a: float = 0.10


class SelectionSpec(BaseModel):
    top_n: int = 3
    cost_bps: float = 20.0
    hysteresis_delta: float = 0.05
    rebalance: int = 5
    horizon: int = 1


class UniverseSpec(BaseModel):
    membership: str = "top2000"
    eligibility: list[str] = Field(default_factory=lambda: ["non_st", "roe>0", "bm>0", "logamt>=floor"])


class StrategyConfig(BaseModel):
    strategy_id: str
    version: int
    owner: str = "local"
    visibility: Literal["private", "published"] = "private"
    factor_subset: list[str]
    normalization: Literal["gauss", "rank", "winz"] = "gauss"
    model: ModelSpec = Field(default_factory=ModelSpec)
    selection: SelectionSpec = Field(default_factory=SelectionSpec)
    universe: UniverseSpec = Field(default_factory=UniverseSpec)


class ModelRecord(BaseModel):
    model_id: str
    strategy_id: str
    strategy_version: int
    status: Literal["training", "frozen", "validated", "published"] = "frozen"
    train_lo: str
    train_hi: str
    factor_cols: list[str]
    factor_cols_hash: str
    weights: list[float]
    delta: float
    top_n: int
    ridge_a: float
    cost_bps: float
    created: str


class Pick(BaseModel):
    rank: int
    symbol: str
    name: str = ""
    score: float


class PickResult(BaseModel):
    strategy_id: str
    model_id: str
    asof: str
    picks: list[Pick]
    eligible_count: int
    degraded_factors: list[str] = Field(default_factory=list)
    hysteresis: bool = True
```

`service/requirements.txt`:
```
fastapi>=0.110
uvicorn>=0.29
httpx>=0.27
pydantic>=2.6
pandas
numpy
pytest
```

- [ ] **Step 4: 跑测试确认通过**

Run: `python -m pytest service/tests/test_models_smoke.py -q`
Expected: PASS(4 passed)

- [ ] **Step 5: 提交**

```bash
git add service/rqs/__init__.py service/rqs/paths.py service/rqs/models.py service/tests/test_models_smoke.py service/requirements.txt
git commit -m "feat(service): scaffold rqs package + pydantic contracts (S-arch MVP task 1)"
```

---

### Task 2: 注册表(文件系统 JSON)+ 导入现有 ridge 为 frozen 模型

**Files:**
- Create: `service/rqs/registry.py`
- Create: `service/rqs/import_paper_ridge.py`
- Create: `service/tests/test_registry.py`

**Interfaces:**
- Consumes: `models.StrategyConfig/ModelRecord`、`paths.REGISTRY_DIR/PAPER_WEIGHTS`。
- Produces:
  - `StrategyRegistry(root=None)`: `.put(cfg)`、`.get(sid, ver) -> StrategyConfig`、`.latest(sid) -> StrategyConfig`
  - `ModelRegistry(root=None)`: `.put(rec)`、`.get(model_id) -> ModelRecord`、`.latest_for_strategy(sid) -> ModelRecord`
  - `import_paper_ridge.main(registry_root=None) -> tuple[str, str]`(返回 (strategy_id, model_id))

- [ ] **Step 1: 写失败测试**

`service/tests/test_registry.py`:
```python
import os, sys, tempfile
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from rqs.registry import StrategyRegistry, ModelRegistry
from rqs.models import StrategyConfig, ModelRecord


def _cfg(ver):
    return StrategyConfig(strategy_id="ridge-gauss", version=ver, factor_subset=["f_bm", "f_mom20"])


def _rec(mid):
    return ModelRecord(model_id=mid, strategy_id="ridge-gauss", strategy_version=1,
                       train_lo="2018-02-06", train_hi="2026-06-04", factor_cols=["f_bm"],
                       factor_cols_hash="h", weights=[0.1], delta=0.05, top_n=3,
                       ridge_a=0.1, cost_bps=20.0, created="2026-06-24T00:00:00")


def test_strategy_put_get_latest():
    with tempfile.TemporaryDirectory() as d:
        reg = StrategyRegistry(root=d)
        reg.put(_cfg(1)); reg.put(_cfg(3)); reg.put(_cfg(2))
        assert reg.get("ridge-gauss", 2).version == 2
        assert reg.latest("ridge-gauss").version == 3


def test_model_put_get_latest_for_strategy():
    with tempfile.TemporaryDirectory() as d:
        reg = ModelRegistry(root=d)
        reg.put(_rec("m1")); reg.put(_rec("m2"))
        assert reg.get("m1").model_id == "m1"
        assert reg.latest_for_strategy("ridge-gauss").model_id in {"m1", "m2"}


def test_import_paper_ridge_creates_records():
    import rqs.import_paper_ridge as imp
    if not os.path.exists(__import__("rqs.paths", fromlist=["PAPER_WEIGHTS"]).PAPER_WEIGHTS):
        import pytest; pytest.skip("no paper_ridge_weights.json present")
    with tempfile.TemporaryDirectory() as d:
        sid, mid = imp.main(registry_root=d)
        assert sid == "ridge-gauss"
        m = ModelRegistry(root=os.path.join(d, "models")).get(mid)
        assert m.status in {"frozen", "published"}
        assert len(m.weights) == len(m.factor_cols)
        assert m.train_hi  # non-empty
```

- [ ] **Step 2: 跑测试确认失败**

Run: `python -m pytest service/tests/test_registry.py -q`
Expected: FAIL(`No module named 'rqs.registry'`)

- [ ] **Step 3: 写实现**

`service/rqs/registry.py`:
```python
import os
from .models import StrategyConfig, ModelRecord
from . import paths


class StrategyRegistry:
    def __init__(self, root=None):
        self.root = root or os.path.join(paths.REGISTRY_DIR, "strategies")
        os.makedirs(self.root, exist_ok=True)

    def _path(self, sid, ver):
        return os.path.join(self.root, f"{sid}.v{ver}.json")

    def put(self, cfg: StrategyConfig):
        with open(self._path(cfg.strategy_id, cfg.version), "w", encoding="utf-8") as f:
            f.write(cfg.model_dump_json(indent=2))

    def get(self, sid, ver) -> StrategyConfig:
        with open(self._path(sid, ver), encoding="utf-8") as f:
            return StrategyConfig.model_validate_json(f.read())

    def _versions(self, sid):
        pre = f"{sid}.v"
        return sorted(int(fn[len(pre):-len(".json")]) for fn in os.listdir(self.root)
                      if fn.startswith(pre) and fn.endswith(".json"))

    def latest(self, sid) -> StrategyConfig:
        vers = self._versions(sid)
        if not vers:
            raise KeyError(sid)
        return self.get(sid, vers[-1])


class ModelRegistry:
    def __init__(self, root=None):
        self.root = root or os.path.join(paths.REGISTRY_DIR, "models")
        os.makedirs(self.root, exist_ok=True)

    def _path(self, model_id):
        return os.path.join(self.root, f"{model_id}.json")

    def put(self, rec: ModelRecord):
        with open(self._path(rec.model_id), "w", encoding="utf-8") as f:
            f.write(rec.model_dump_json(indent=2))

    def get(self, model_id) -> ModelRecord:
        with open(self._path(model_id), encoding="utf-8") as f:
            return ModelRecord.model_validate_json(f.read())

    def _all(self):
        for fn in os.listdir(self.root):
            if fn.endswith(".json"):
                yield self.get(fn[:-len(".json")])

    def latest_for_strategy(self, sid) -> ModelRecord:
        cands = [m for m in self._all() if m.strategy_id == sid]
        if not cands:
            raise KeyError(sid)
        return max(cands, key=lambda m: m.created)
```

`service/rqs/import_paper_ridge.py`:
```python
"""Import the existing frozen paper_ridge_weights.json into the registries as the
first ridge StrategyConfig + frozen ModelRecord. Adopt, do NOT retrain."""
import hashlib
import json
import os

from . import paths
from .models import StrategyConfig, ModelRecord, ModelSpec, SelectionSpec
from .registry import StrategyRegistry, ModelRegistry

STRATEGY_ID = "ridge-gauss"


def main(registry_root=None):
    with open(paths.PAPER_WEIGHTS, encoding="utf-8") as f:
        meta = json.load(f)
    sroot = os.path.join(registry_root, "strategies") if registry_root else None
    mroot = os.path.join(registry_root, "models") if registry_root else None

    cfg = StrategyConfig(
        strategy_id=STRATEGY_ID, version=1, owner="local", visibility="private",
        factor_subset=list(meta["factor_cols"]), normalization="gauss",
        model=ModelSpec(type="ridge", ridge_a=float(meta["ridge_a"])),
        selection=SelectionSpec(top_n=int(meta["top_n"]), cost_bps=float(meta["cost_bps"]),
                                hysteresis_delta=float(meta["delta"]), rebalance=5, horizon=1),
    )
    StrategyRegistry(root=sroot).put(cfg)

    cols_hash = hashlib.sha256("|".join(meta["factor_cols"]).encode()).hexdigest()[:16]
    model_id = f"{STRATEGY_ID}-v1-{meta['train_hi']}"
    rec = ModelRecord(
        model_id=model_id, strategy_id=STRATEGY_ID, strategy_version=1, status="frozen",
        train_lo=meta["train_lo"], train_hi=meta["train_hi"], factor_cols=list(meta["factor_cols"]),
        factor_cols_hash=cols_hash, weights=[float(x) for x in meta["weights"]],
        delta=float(meta["delta"]), top_n=int(meta["top_n"]), ridge_a=float(meta["ridge_a"]),
        cost_bps=float(meta["cost_bps"]), created=meta.get("created", "1970-01-01T00:00:00"),
    )
    ModelRegistry(root=mroot).put(rec)
    return STRATEGY_ID, model_id


if __name__ == "__main__":
    sid, mid = main()
    print(f"imported strategy={sid} model={mid}")
```

- [ ] **Step 4: 跑测试确认通过**

Run: `python -m pytest service/tests/test_registry.py -q`
Expected: PASS(3 passed;若无 weights.json 则第 3 个 skip)

- [ ] **Step 5: 提交**

```bash
git add service/rqs/registry.py service/rqs/import_paper_ridge.py service/tests/test_registry.py
git commit -m "feat(service): filesystem registries + import frozen ridge (S-arch MVP task 2)"
```

---

### Task 3: S1 数据新鲜度

**Files:**
- Create: `service/rqs/data.py`
- Create: `service/tests/test_data.py`

**Interfaces:**
- Consumes: `paths.KDAY`。
- Produces: `data.latest_data_date(kday_dir=None, refs=None) -> str | None`(返回最新交易日 "YYYY-MM-DD";读若干参考股取其末日的众数/最大值,缺失→None)。

- [ ] **Step 1: 写失败测试**

`service/tests/test_data.py`:
```python
import os, sys
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import pandas as pd
from rqs.data import latest_data_date


def test_latest_from_temp_kday(tmp_path):
    for sym, last in [("sh600519", "2026-06-23"), ("sh601398", "2026-06-23")]:
        pd.DataFrame({"time": [f"{last} 15:00:00"], "open": [1.0], "high": [1.0],
                      "low": [1.0], "close": [1.0], "volume": [1], "amount": [1.0],
                      "turn": [0.1], "pctChg": [0.0]}).to_csv(tmp_path / f"{sym}.csv", index=False)
    assert latest_data_date(kday_dir=str(tmp_path), refs=["sh600519", "sh601398"]) == "2026-06-23"


def test_missing_returns_none(tmp_path):
    assert latest_data_date(kday_dir=str(tmp_path), refs=["sh600519"]) is None
```

- [ ] **Step 2: 跑测试确认失败**

Run: `python -m pytest service/tests/test_data.py -q`
Expected: FAIL(`No module named 'rqs.data'`)

- [ ] **Step 3: 写实现**

`service/rqs/data.py`:
```python
import os
import pandas as pd
from . import paths

_DEFAULT_REFS = ["sh600519", "sh601398", "sz000001", "sz300750"]


def _last_date(path):
    try:
        s = pd.read_csv(path, usecols=["time"])["time"]
        return str(s.iloc[-1])[:10] if len(s) else None
    except (OSError, ValueError, KeyError):
        return None


def latest_data_date(kday_dir=None, refs=None):
    """Latest trading date available locally, as the max last-date over reference symbols.
    Returns 'YYYY-MM-DD' or None when no reference file is readable."""
    kday_dir = kday_dir or paths.KDAY
    refs = refs or _DEFAULT_REFS
    dates = [d for d in (_last_date(os.path.join(kday_dir, f"{s}.csv")) for s in refs) if d]
    return max(dates) if dates else None
```

- [ ] **Step 4: 跑测试确认通过**

Run: `python -m pytest service/tests/test_data.py -q`
Expected: PASS(2 passed)

- [ ] **Step 5: 提交**

```bash
git add service/rqs/data.py service/tests/test_data.py
git commit -m "feat(service): S1 data freshness probe (S-arch MVP task 3)"
```

---

### Task 4: S2 因子 as-of 截面

**Files:**
- Create: `service/rqs/factors.py`
- Create: `service/tests/test_factors.py`

**Interfaces:**
- Consumes: `paths.SCRIPTS/ASOF_CSV/REPO`;子进程调用 `scripts/build_factor_matrix.py --asof <date>`。
- Produces: `factors.asof_cross_section(date, factor_set=None, rebuild=True, csv_path=None) -> pandas.DataFrame`(含列 `date,symbol,<factors>,fwd_ret_5d`,仅该 `date` 行)。

- [ ] **Step 1: 写失败测试**

`service/tests/test_factors.py`:
```python
import os, sys
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import pandas as pd
from rqs.factors import asof_cross_section


def test_reads_and_filters_by_date(tmp_path):
    csv = tmp_path / "asof.csv"
    pd.DataFrame({"date": ["2026-06-24", "2026-06-23"], "symbol": ["sz000039", "sh600000"],
                  "f_bm": [1.0, 2.0], "f_mom20": [0.1, 0.2], "fwd_ret_5d": [None, None]}
                 ).to_csv(csv, index=False)
    df = asof_cross_section("2026-06-24", rebuild=False, csv_path=str(csv))
    assert list(df["symbol"]) == ["sz000039"]
    assert df["date"].iloc[0] == "2026-06-24"


def test_factor_subset_projection(tmp_path):
    csv = tmp_path / "asof.csv"
    pd.DataFrame({"date": ["2026-06-24"], "symbol": ["sz000039"], "f_bm": [1.0],
                  "f_mom20": [0.1], "f_roe": [0.3], "fwd_ret_5d": [None]}).to_csv(csv, index=False)
    df = asof_cross_section("2026-06-24", factor_set=["f_bm"], rebuild=False, csv_path=str(csv))
    assert "f_bm" in df.columns and "symbol" in df.columns
```

- [ ] **Step 2: 跑测试确认失败**

Run: `python -m pytest service/tests/test_factors.py -q`
Expected: FAIL(`No module named 'rqs.factors'`)

- [ ] **Step 3: 写实现**

`service/rqs/factors.py`:
```python
import os
import subprocess
import sys
import pandas as pd
from . import paths


def asof_cross_section(date, factor_set=None, rebuild=True, csv_path=None):
    """Return the as-of cross-section for `date` as a DataFrame.

    rebuild=True runs scripts/build_factor_matrix.py --asof <date> (offline; reads
    only local data/baostock/kday) to (re)write factors_asof.csv, then loads it.
    rebuild=False loads an existing csv (csv_path or paths.ASOF_CSV) — used in unit
    tests and when the panel is already built for `date`.
    """
    path = csv_path or paths.ASOF_CSV
    if rebuild:
        subprocess.run([sys.executable, os.path.join(paths.SCRIPTS, "build_factor_matrix.py"),
                        "--asof", date], cwd=paths.REPO, check=True)
    df = pd.read_csv(path, dtype={"symbol": str})
    df = df[df["date"].astype(str) == date].copy()
    if factor_set:
        keep = ["date", "symbol", *[c for c in factor_set if c in df.columns]]
        if "fwd_ret_5d" in df.columns:
            keep.append("fwd_ret_5d")
        df = df[keep]
    return df.reset_index(drop=True)
```

- [ ] **Step 4: 跑测试确认通过**

Run: `python -m pytest service/tests/test_factors.py -q`
Expected: PASS(2 passed)

- [ ] **Step 5: 提交**

```bash
git add service/rqs/factors.py service/tests/test_factors.py
git commit -m "feat(service): S2 asof_cross_section wrapping build_factor_matrix (S-arch MVP task 4)"
```

---

### Task 5: S6 选股(复用 select_picks)

**Files:**
- Create: `service/rqs/selection.py`
- Create: `service/tests/test_selection.py`

**Interfaces:**
- Consumes: `scripts/eval_ridge._eligible`、`scripts/paper_ridge.select_picks`、`scripts/test_norm_hysteresis.norm_gauss`、`models.ModelRecord/Pick/PickResult`。
- Produces: `selection.asof_pick(model, matrix_df, st_set, names=None, prev_picks=None, hysteresis=True) -> PickResult`。

- [ ] **Step 1: 写失败测试**

`service/tests/test_selection.py`:
```python
import os, sys
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(
    os.path.abspath(__file__)))), "scripts"))
import numpy as np
import pandas as pd
from build_factor_matrix import FACTOR_COLS
from rqs.models import ModelRecord
from rqs.selection import asof_pick


def _model(weights):
    return ModelRecord(model_id="m", strategy_id="ridge-gauss", strategy_version=1,
                       train_lo="2018-02-06", train_hi="2026-06-04", factor_cols=list(FACTOR_COLS),
                       factor_cols_hash="h", weights=list(weights), delta=0.05, top_n=2,
                       ridge_a=0.1, cost_bps=20.0, created="2026-06-24T00:00:00")


def _rows(specs):
    out = []
    for sym, mom in specs:
        d = {c: 0.0 for c in FACTOR_COLS}
        d.update({"f_roe": 1.0, "f_bm": 1.0, "f_logamt": 18.0, "f_mom20": mom})
        d.update({"date": "2026-06-24", "symbol": sym, "fwd_ret_5d": np.nan})
        out.append(d)
    return pd.DataFrame(out)


def test_picks_top_n_by_mom():
    w = np.zeros(len(FACTOR_COLS)); w[FACTOR_COLS.index("f_mom20")] = 1.0
    df = _rows([("szA", 0.5), ("szB", 0.1), ("szC", 0.9)])
    res = asof_pick(_model(w), df, st_set=set(), hysteresis=False)
    assert [p.symbol for p in res.picks] == ["szC", "szA"]
    assert res.eligible_count == 3


def test_hysteresis_bonus_holds_incumbent():
    w = np.zeros(len(FACTOR_COLS)); w[FACTOR_COLS.index("f_mom20")] = 1.0
    df = _rows([("szA", 0.50), ("szB", 0.49)])
    base = asof_pick(_model(w), df, st_set=set(), prev_picks=["szB"], hysteresis=False)
    held = asof_pick(_model(w), df, st_set=set(), prev_picks=["szB"], hysteresis=True)
    assert base.picks[0].symbol == "szA"
    assert held.picks[0].symbol == "szB"  # +delta flips it
```

- [ ] **Step 2: 跑测试确认失败**

Run: `python -m pytest service/tests/test_selection.py -q`
Expected: FAIL(`No module named 'rqs.selection'`)

- [ ] **Step 3: 写实现**

`service/rqs/selection.py`:
```python
import os
import sys
import numpy as np
from . import paths
from .models import ModelRecord, Pick, PickResult

sys.path.insert(0, paths.SCRIPTS)
import eval_ridge as er          # noqa: E402
import paper_ridge as pr         # noqa: E402
from test_norm_hysteresis import norm_gauss  # noqa: E402


def _name_st_mask(symbols, names):
    """Mirror paper_ridge.asof_pick's name-based ST double-guard."""
    return symbols.map(lambda s: ("ST" in str(names.get(s, "")).upper()) or ("退" in str(names.get(s, ""))))


def asof_pick(model: ModelRecord, matrix_df, st_set, names=None, prev_picks=None, hysteresis=True) -> PickResult:
    names = names or {}
    cols = list(model.factor_cols)
    elig = er._eligible(matrix_df, st_set)
    if names is not None and len(elig):
        elig = elig[~_name_st_mask(elig["symbol"], names)]
    degraded = [c for c in cols if c in matrix_df.columns and matrix_df[c].isna().all()]

    w = np.asarray(model.weights, float)
    delta = model.delta if hysteresis else 0.0
    prev = list(prev_picks or [])
    picks = pr.select_picks(elig, w, prev, delta, model.top_n)

    G = norm_gauss(elig[cols].to_numpy(float))
    score = G @ w
    if delta > 0.0 and prev:
        score = score + delta * elig["symbol"].isin(set(prev)).to_numpy().astype(float)
    gi = elig.assign(_score=score).sort_values(["_score", "symbol"], ascending=[False, True])
    rank_by_sym = {s: i for i, s in enumerate(gi["symbol"])}
    score_by_sym = dict(zip(gi["symbol"], gi["_score"]))

    rows = [Pick(rank=i + 1, symbol=s, name=names.get(s, ""), score=float(score_by_sym[s]))
            for i, s in enumerate(picks)]
    asof = str(matrix_df["date"].iloc[0]) if len(matrix_df) else ""
    return PickResult(strategy_id=model.strategy_id, model_id=model.model_id, asof=asof,
                      picks=rows, eligible_count=int(len(elig)), degraded_factors=degraded,
                      hysteresis=hysteresis)
```

- [ ] **Step 4: 跑测试确认通过**

Run: `python -m pytest service/tests/test_selection.py -q`
Expected: PASS(2 passed)

- [ ] **Step 5: 提交**

```bash
git add service/rqs/selection.py service/tests/test_selection.py
git commit -m "feat(service): S6 asof_pick reusing select_picks (S-arch MVP task 5)"
```

---

### Task 6: 黄金平价测试(验收核心)

**Files:**
- Create: `service/tests/test_parity.py`

**Interfaces:**
- Consumes: `data.latest_data_date`、`factors.asof_cross_section`、`selection.asof_pick`、`registry.ModelRegistry`、`import_paper_ridge`、`scripts/paper_ridge.asof_pick`。

**说明:** 集成测试(~1–3 分钟:离线建 as-of 面板)。无真数据/权重时 skip。锁定"新脊柱 == `paper_ridge.py --asof` 逐票"。

- [ ] **Step 1: 写测试**

`service/tests/test_parity.py`:
```python
import os, sys, tempfile
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(
    os.path.abspath(__file__)))), "scripts"))
import pandas as pd
import pytest
from rqs import paths
from rqs.data import latest_data_date
from rqs.factors import asof_cross_section
from rqs.selection import asof_pick
from rqs.registry import ModelRegistry
import rqs.import_paper_ridge as imp


@pytest.mark.integration
def test_parity_with_paper_ridge():
    if not os.path.exists(paths.PAPER_WEIGHTS):
        pytest.skip("no frozen weights")
    d = latest_data_date()
    if d is None:
        pytest.skip("no local kday")

    with tempfile.TemporaryDirectory() as reg:
        sid, mid = imp.main(registry_root=reg)
        model = ModelRegistry(root=os.path.join(reg, "models")).get(mid)

    df = asof_cross_section(d, factor_set=model.factor_cols, rebuild=True)
    if len(df) < model.top_n:
        pytest.skip(f"eligible pool too small for {d}")

    st_set = set(pd.read_csv(paths.ST_PATH)["symbol"]) if os.path.exists(paths.ST_PATH) else set()
    names = {}
    if os.path.exists(paths.NAMES_PATH):
        nd = pd.read_csv(paths.NAMES_PATH, dtype=str)
        names = dict(zip(nd["symbol"], nd["name"]))

    got = [p.symbol for p in asof_pick(model, df, st_set, names, hysteresis=False).picks]

    import paper_ridge as pr
    want = pr.asof_pick(pr.load_weights(), st_set, d, top_n=model.top_n, hysteresis=False)
    assert got == want
```

- [ ] **Step 2: 跑测试**

Run: `python -m pytest service/tests/test_parity.py -q -m integration`
Expected: PASS(1 passed;无数据/权重则 skipped)。**若 FAIL**:对照 `selection.asof_pick` 与 `paper_ridge.asof_pick` 的 eligibility / 名称 ST 闸 / 列序差异并修齐(parity 是验收红线)。

- [ ] **Step 3: 提交**

```bash
git add service/tests/test_parity.py
git commit -m "test(service): golden parity vs paper_ridge --asof (S-arch MVP task 6)"
```

---

### Task 7: S7 FastAPI 端点 + 新鲜度守卫

**Files:**
- Create: `service/rqs/api.py`
- Create: `service/tests/test_api.py`

**Interfaces:**
- Consumes: `registry.ModelRegistry`、`factors.asof_cross_section`、`selection.asof_pick`、`data.latest_data_date`。
- Produces: FastAPI `app`;`GET /strategies/{sid}/picks?asof=YYYY-MM-DD&hysteresis=bool` → `PickResult` JSON;数据不新鲜→409;合格池不足→422;策略无模型→404。`GET /healthz` → `{"ok": true}`。

- [ ] **Step 1: 写失败测试**

`service/tests/test_api.py`:
```python
import os, sys
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import pandas as pd
import pytest
from fastapi.testclient import TestClient
import rqs.api as api
from rqs.models import ModelRecord, PickResult, Pick


def _client(monkeypatch, latest="2026-06-24", model=True, pool_ok=True):
    monkeypatch.setattr(api, "latest_data_date", lambda: latest)
    if model:
        rec = ModelRecord(model_id="m", strategy_id="ridge-gauss", strategy_version=1,
                          train_lo="2018-02-06", train_hi="2026-06-04", factor_cols=["f_bm"],
                          factor_cols_hash="h", weights=[0.1], delta=0.05, top_n=3,
                          ridge_a=0.1, cost_bps=20.0, created="2026-06-24T00:00:00")
        monkeypatch.setattr(api, "_load_model", lambda sid: rec)
    else:
        def _raise(sid): raise KeyError(sid)
        monkeypatch.setattr(api, "_load_model", _raise)
    monkeypatch.setattr(api, "asof_cross_section",
                        lambda date, factor_set=None, rebuild=True: pd.DataFrame(
                            {"date": ["2026-06-24"] * (3 if pool_ok else 0)}))
    monkeypatch.setattr(api, "_pick", lambda model, df, hyst: PickResult(
        strategy_id="ridge-gauss", model_id="m", asof="2026-06-24",
        picks=[Pick(rank=1, symbol="sz000039", name="中集集团", score=0.1)],
        eligible_count=3, hysteresis=hyst))
    return TestClient(api.app)


def test_healthz(monkeypatch):
    assert _client(monkeypatch).get("/healthz").json() == {"ok": True}


def test_picks_ok(monkeypatch):
    r = _client(monkeypatch).get("/strategies/ridge-gauss/picks?asof=2026-06-24")
    assert r.status_code == 200
    assert r.json()["picks"][0]["symbol"] == "sz000039"


def test_stale_data_409(monkeypatch):
    r = _client(monkeypatch, latest="2026-06-23").get("/strategies/ridge-gauss/picks?asof=2026-06-24")
    assert r.status_code == 409


def test_no_model_404(monkeypatch):
    r = _client(monkeypatch, model=False).get("/strategies/none/picks?asof=2026-06-24")
    assert r.status_code == 404


def test_small_pool_422(monkeypatch):
    r = _client(monkeypatch, pool_ok=False).get("/strategies/ridge-gauss/picks?asof=2026-06-24")
    assert r.status_code == 422
```

- [ ] **Step 2: 跑测试确认失败**

先装依赖:`python -m pip install -r service/requirements.txt`
Run: `python -m pytest service/tests/test_api.py -q`
Expected: FAIL(`No module named 'rqs.api'`)

- [ ] **Step 3: 写实现**

`service/rqs/api.py`:
```python
import os
import sys
import pandas as pd
from fastapi import FastAPI, HTTPException
from . import paths
from .data import latest_data_date
from .factors import asof_cross_section
from .registry import ModelRegistry
from .selection import asof_pick

sys.stdout.reconfigure(encoding="utf-8")
app = FastAPI(title="rquant selection service")


def _load_model(sid):
    return ModelRegistry().latest_for_strategy(sid)


def _load_st_set():
    return set(pd.read_csv(paths.ST_PATH)["symbol"]) if os.path.exists(paths.ST_PATH) else set()


def _load_names():
    if not os.path.exists(paths.NAMES_PATH):
        return {}
    nd = pd.read_csv(paths.NAMES_PATH, dtype=str)
    return dict(zip(nd["symbol"], nd["name"]))


def _pick(model, df, hyst):
    return asof_pick(model, df, _load_st_set(), _load_names(), hysteresis=hyst)


@app.get("/healthz")
def healthz():
    return {"ok": True}


@app.get("/strategies/{sid}/picks")
def get_picks(sid: str, asof: str, hysteresis: bool = True):
    latest = latest_data_date()
    if latest is None or asof > latest:
        raise HTTPException(status_code=409, detail=f"data not fresh to {asof} (latest={latest})")
    try:
        model = _load_model(sid)
    except KeyError:
        raise HTTPException(status_code=404, detail=f"no model for strategy {sid}")
    df = asof_cross_section(asof, factor_set=model.factor_cols, rebuild=True)
    if len(df) < model.top_n:
        raise HTTPException(status_code=422, detail=f"eligible pool too small for {asof}")
    return _pick(model, df, hysteresis).model_dump()
```

- [ ] **Step 4: 跑测试确认通过**

Run: `python -m pytest service/tests/test_api.py -q`
Expected: PASS(5 passed)

- [ ] **Step 5: 提交**

```bash
git add service/rqs/api.py service/tests/test_api.py
git commit -m "feat(service): S7 FastAPI picks endpoint + freshness guard (S-arch MVP task 7)"
```

---

### Task 8: 容器化 + README + 全套测试

**Files:**
- Create: `service/Dockerfile`
- Create: `service/README.md`
- Create: `docker-compose.yml`

**Interfaces:**
- Produces: 可 `docker compose up` 起 API 容器(端口 8000);`GET /healthz` 通。

- [ ] **Step 1: 写实现**

`service/Dockerfile`:
```dockerfile
FROM python:3.13-slim
WORKDIR /app
COPY service/requirements.txt /app/service/requirements.txt
RUN pip install --no-cache-dir -r /app/service/requirements.txt
COPY scripts /app/scripts
COPY service /app/service
COPY data /app/data
ENV PYTHONPATH=/app/service
CMD ["uvicorn", "rqs.api:app", "--host", "0.0.0.0", "--port", "8000"]
```

`docker-compose.yml`:
```yaml
services:
  api:
    build: { context: ., dockerfile: service/Dockerfile }
    ports: ["8000:8000"]
    volumes:
      - ./data:/app/data
    restart: unless-stopped
```

`service/README.md`:
```markdown
# rquant 选股服务(MVP 脊柱)

服务化架构 spec(`docs/superpowers/specs/2026-06-24-factor-weight-selection-service-arch-design.md`)
流②(选股读路径)的最小实现:注册表 → 因子 as-of → 选股 → API。复用 scripts/ 下已验证逻辑。

## 一次性导入冻结 ridge
    python -m service.rqs.import_paper_ridge   # 或 cd service && python -m rqs.import_paper_ridge

## 本地跑
    python -m pip install -r service/requirements.txt
    cd service && uvicorn rqs.api:app --reload
    curl "http://127.0.0.1:8000/strategies/ridge-gauss/picks?asof=2026-06-24&hysteresis=false"

## 容器
    docker compose up --build
    curl http://127.0.0.1:8000/healthz

## 测试
    python -m pytest service/tests -q              # 快测
    python -m pytest service/tests -q -m integration  # 含黄金平价(~1-3min)

## 边界(MVP)
- 单用户、单策略、只读路径;训练/采集中心/多租户见 spec 阶段 2+。
- 只读 data/baostock/kday,绝不写当日 bar 进 canonical(补数还原属阶段 2 采集中心)。
- 注册表后端为文件系统 JSON;Postgres 留到阶段 2(Registry 接口即替换接缝)。
```

- [ ] **Step 2: 注册 pytest integration marker(避免告警)**

Create `service/pytest.ini`:
```ini
[pytest]
markers =
    integration: 真数据/较慢的集成测试
```

- [ ] **Step 3: 跑全套快测**

Run: `python -m pytest service/tests -q -m "not integration"`
Expected: PASS(task 1/2/3/4/5/7 的单测全绿)

- [ ] **Step 4: 起容器冒烟**

```bash
docker compose up --build -d
curl -s http://127.0.0.1:8000/healthz
docker compose down
```
Expected: `{"ok":true}`

- [ ] **Step 5: 提交**

```bash
git add service/Dockerfile service/README.md service/pytest.ini docker-compose.yml
git commit -m "feat(service): containerize MVP selection service + docs (S-arch MVP task 8)"
```

---

## Self-Review(对照 spec)

**Spec coverage(MVP §7 七个动作):** ① 导入冻结权重→Task 2;② ridge 配置登记为 strategy→Task 2;③ `build_factor_matrix --asof` 包成 S2→Task 4;④ `paper_ridge` 选股包成 S6→Task 5;⑤ 数据读/新鲜度(S1 最小)→Task 3;⑥ `GET /strategies/{id}/picks`→Task 7;⑦ 桌面 PaperRidge 改调 API→**本计划之外(下一片,跨 TS/Rust)**,已在范围中声明。出口标准(golden parity)→Task 6。容器/运维从简→Task 8。

**与 spec 的有意偏差(待评审确认):**
- 注册表后端 MVP 用文件系统 JSON 而非 Postgres(spec 阶段 0 提 Postgres)。理由:MVP 最薄、零运维;Registry 接口为阶段 2 换 Postgres 的接缝。若要求 MVP 即上 Postgres,在 Task 2 增建 schema + 用 SQLAlchemy 后端。
- 采集中心(调度/看门狗/多源回退)不在 MVP;MVP 只读已落地数据(spec §7 已明确阶段 2 才建)。

**Placeholder scan:** 无 TBD/TODO;每步含完整代码与命令。

**Type consistency:** `asof_cross_section(date, factor_set, rebuild, csv_path)`、`asof_pick(model, matrix_df, st_set, names, prev_picks, hysteresis)`、`ModelRegistry.latest_for_strategy/get`、`latest_data_date(kday_dir, refs)` 在 Task 4/5/6/7 引用一致;`PickResult` 字段(picks/eligible_count/degraded_factors/hysteresis)Task 1 定义、Task 5/7 消费一致。

**关键风险:** Task 6 平价若不齐 → 多因名称 ST 双保险或列序;已在 selection 内镜像 `paper_ridge.asof_pick` 的名称闸,逐票对齐为红线。
