# Claude-in-the-loop 选股树迭代 harness — 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development 或 executing-plans 逐任务实现。步骤用 `- [ ]`。

**Goal:** 落地 `scripts/iterate.py` 轮驱动 + 迭代账本 + /loop prompt，让 Claude 全自治多轮快速迭代横截面日频选股树。

**Architecture:** `iterate.py` 复用 `daily_eval.run_once`（screen 回测 gross/net→JSON），加分层(Tier-1/2)、过拟合旗标、裁决、账本追加、轮卡打印。纯函数(裁决/旗标/break-even/符号翻转)单测；I/O 走 smoke。零引擎/Rust 改动。

**Tech Stack:** Python 3.13 + pandas（已装）；复用 scripts/daily_eval.py；screen 引擎 + baostock 数据集。

## Global Constraints
- 净成本 20bps；break-even 门槛 = 2×成本 = 40bps。
- OOS 金标准：regime 标签含 "OOS" 为样本外窗。
- PASS 需全满足：毛超额>0 且 净OOS超额>0 且 净Sharpe>0 且 break-even≥40bps 且 (Tier-2)无符号翻转。否则 FALSIFIED。
- 位置无关路径（派生 REPO_ROOT）；UTF-8 stdout（`sys.stdout.reconfigure`）。
- 数据 gitignored；账本 md 入库、jsonl 入 `.iter/`(gitignore)。
- §5.3：脚本不调参凑数；证伪=合法产出；账本防重复。

---

### Task 1: iterate.py 纯函数核心（裁决/旗标/break-even/符号翻转）

**Files:** Create `scripts/iterate.py`（含纯函数）, `scripts/test_iterate.py`

**Interfaces produced:**
- `break_even(gross_ex: float, net_ex: float, cost: float) -> float|None`
- `regime_excess(report: dict, oos: bool) -> float|None`（按 label 是否含 "OOS"）
- `detect_sign_flip(net_excesses: list[float]) -> bool`
- `judge(g: dict, n: dict, sweep: list[float]|None) -> tuple[str, list[str], dict]` → (verdict, flags, metrics)

- [ ] **Step 1: 写失败测试** `scripts/test_iterate.py`
```python
import math
from iterate import break_even, regime_excess, detect_sign_flip, judge

def _rep(excess, sharpe, regimes):  # 最小 screen 报告
    return {"excess_return": excess, "risk": {"sharpe": sharpe},
            "regime_slices": [{"label": k, "excess": v} for k, v in regimes]}

def test_break_even():
    assert math.isclose(break_even(0.20, 0.10, 20.0), 40.0, rel_tol=1e-9)  # decay .10 → 20*.2/.1
    assert break_even(-0.1, -0.3, 20.0) is None   # 毛≤0
    assert break_even(0.0, 0.0, 20.0) is None

def test_regime_excess():
    n = _rep(-0.1, 0.2, [("train", 0.05), ("2024-26_OOS", -0.03)])
    assert math.isclose(regime_excess(n, oos=True), -0.03)
    assert math.isclose(regime_excess(n, oos=False), 0.05)

def test_sign_flip():
    assert detect_sign_flip([0.1, -0.02, 0.05]) is True
    assert detect_sign_flip([0.1, 0.05, 0.2]) is False

def test_judge_pass():
    g = _rep(0.30, None, [("train", 0.2), ("OOS", 0.12)])
    n = _rep(0.18, 1.1, [("train", 0.15), ("OOS", 0.09)])
    v, flags, _ = judge(g, n, sweep=[0.09, 0.08, 0.11])   # be=20*.30/.12=50>=40, no flip
    assert v == "PASS" and flags == []

def test_judge_falsified_oos():
    g = _rep(0.30, None, [("train", 0.2), ("OOS", 0.12)])
    n = _rep(-0.02, 0.8, [("train", 0.1), ("OOS", -0.04)])   # net OOS<0 + in-sample-only
    v, flags, _ = judge(g, n, sweep=None)
    assert v == "FALSIFIED" and "net-OOS<=0" in flags and "in-sample-only" in flags

def test_judge_signflip_falsifies():
    g = _rep(0.30, None, [("train", 0.2), ("OOS", 0.12)])
    n = _rep(0.18, 1.1, [("train", 0.15), ("OOS", 0.09)])
    v, flags, _ = judge(g, n, sweep=[0.09, -0.03, 0.11])   # flip
    assert v == "FALSIFIED" and "sign-flip" in flags
```

- [ ] **Step 2: 运行确认失败** `cd scripts && python -m pytest test_iterate.py -q` → FAIL (ImportError)

- [ ] **Step 3: 实现纯函数**（`scripts/iterate.py` 顶部）
```python
#!/usr/bin/env python3
"""Claude-in-the-loop 选股树迭代轮驱动。见 docs/superpowers/specs/2026-06-18-iteration-harness-design.md"""
import argparse, json, os, sys, time
try: sys.stdout.reconfigure(encoding="utf-8")
except Exception: pass
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import daily_eval as de

COST = 20.0
BE_MIN = 40.0      # 2×成本
OOS_TAG = "OOS"

def break_even(gross_ex, net_ex, cost):
    decay = gross_ex - net_ex
    return cost * gross_ex / decay if (decay > 0 and gross_ex > 0) else None

def regime_excess(report, oos):
    for s in report.get("regime_slices", []):
        if (OOS_TAG in s["label"]) == oos:
            return s["excess"]
    return None

def detect_sign_flip(net_excesses):
    xs = [x for x in net_excesses if x is not None]
    return any(x > 0 for x in xs) and any(x < 0 for x in xs)

def judge(g, n, sweep):
    gx, nx = g["excess_return"], n["excess_return"]
    nsh = (n.get("risk") or {}).get("sharpe")
    oos, tr = regime_excess(n, True), regime_excess(n, False)
    be = break_even(gx, nx, COST)
    flags = []
    if gx <= 0: flags.append("gross-excess<=0")
    if oos is not None and oos <= 0: flags.append("net-OOS<=0")
    if nsh is not None and nsh <= 0: flags.append("net-sharpe<=0")
    if tr is not None and oos is not None and tr > 0 >= oos: flags.append("in-sample-only")
    if be is None or be < BE_MIN: flags.append(f"break-even<{int(BE_MIN)}bps")
    if sweep is not None and detect_sign_flip(sweep): flags.append("sign-flip")
    passed = (gx > 0 and oos is not None and oos > 0 and nsh is not None and nsh > 0
              and be is not None and be >= BE_MIN
              and (sweep is None or not detect_sign_flip(sweep)))
    metrics = {"gross_ex": gx, "net_ex": nx, "net_oos_ex": oos, "net_train_ex": tr,
               "net_sharpe": nsh, "break_even": be}
    return ("PASS" if passed else "FALSIFIED"), flags, metrics
```

- [ ] **Step 4: 运行确认通过** `cd scripts && python -m pytest test_iterate.py -q` → 6 passed

- [ ] **Step 5: 提交**
```bash
git add scripts/iterate.py scripts/test_iterate.py
git commit -m "feat(iterate): harness verdict/overfit-flag/break-even core + tests"
```

---

### Task 2: iterate.py I/O（跑 Tier-1/2 + 轮卡 + 账本追加）

**Files:** Modify `scripts/iterate.py`（加 main + run/card/ledger）

**Interfaces consumed:** `daily_eval.run_once(cfg, cost, frm, to, warmup, window, top, out, universe, membership)` → report dict（已存在）；`daily_eval.REPO_ROOT`, `daily_eval.RUNS`。
**Produces:** CLI `python scripts/iterate.py <config> --note "h" [--axis daily|intraday] [--label L] [--top N] [--from .. --to ..]`；追加 `.iter/ledger.jsonl` + `docs/superpowers/iteration-ledger.md` 表行；打印轮卡。

- [ ] **Step 1: 实现 main + 轮卡 + 账本**（append 到 iterate.py）
```python
LEDGER_MD = os.path.join(de.REPO_ROOT, "docs", "superpowers", "iteration-ledger.md")
LEDGER_JSONL = os.path.join(de.REPO_ROOT, ".iter", "ledger.jsonl")
AXES = {  # universe + 窗口（spec §6）
    "daily":    {"universe": "data/baostock/universe_baostock_day.csv", "frm": "2018-01-01", "to": "2026-06-30", "warmup": 60, "window": 60},
    "intraday": {"universe": "data/baostock/universe_intraday_day.csv", "frm": "2021-01-01", "to": "2026-06-30", "warmup": 5,  "window": 10},
}

def _next_round():
    if not os.path.exists(LEDGER_JSONL): return 1
    return sum(1 for _ in open(LEDGER_JSONL, encoding="utf-8")) + 1

def _prior_best_oos():
    best = None
    if os.path.exists(LEDGER_JSONL):
        for line in open(LEDGER_JSONL, encoding="utf-8"):
            try: r = json.loads(line)
            except Exception: continue
            o = r.get("net_oos_ex")
            if o is not None and (best is None or o > best): best = o
    return best

def run(cfg, axis, top, label, note):
    a = AXES[axis]; uni = os.path.join(de.REPO_ROOT, a["universe"])
    os.makedirs(de.RUNS, exist_ok=True)
    g_out = os.path.join(de.RUNS, f"iter_{label}_gross.json")
    n_out = os.path.join(de.RUNS, f"iter_{label}_net.json")
    g = de.run_once(cfg, 0.0,  a["frm"], a["to"], a["warmup"], a["window"], top, g_out, uni, "none")
    n = de.run_once(cfg, COST, a["frm"], a["to"], a["warmup"], a["window"], top, n_out, uni, "none")
    return g, n, a

def tier2_sweep(cfg, axis, label):  # net excess across top×reb grid
    a = AXES[axis]; uni = os.path.join(de.REPO_ROOT, a["universe"]); out = []
    for top in (30, 50, 100):
        for reb in (1, 5):
            o = os.path.join(de.RUNS, f"iter_{label}_s{top}_{reb}.json")
            # rebalance via separate call: daily_eval.run_once 固定 reb=1；扫 reb 需直接调引擎
            rep = _screen(cfg, COST, a, top, reb, o, uni)
            out.append(rep["excess_return"])
    return out

def _screen(cfg, cost, a, top, reb, out, uni):  # 直接调引擎(支持 --rebalance 扫描)
    import subprocess
    cmd = [de.BIN, "screen", "--backtest", "--universe", uni, "--config", cfg,
           "--rebalance", str(reb), "--warmup", str(a["warmup"]), "--window", str(a["window"]),
           "--cost-bps", str(cost), "--from", a["frm"], "--to", a["to"], "--top", str(top), "--out", out]
    subprocess.run(cmd, cwd=de.REPO_ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, encoding="utf-8", errors="replace")
    return json.load(open(out, encoding="utf-8"))

def card(rnd, label, axis, note, g, n, a, verdict, flags, m, sweep, prior):
    def f(x): return "—" if x is None else f"{x:+.4f}"
    L = [f"=== ITER round {rnd} · {label} (axis={axis}) ===",
         f"hypothesis : {note}",
         f"window train/OOS via config regimes   cost {int(COST)}bps   top {a.get('top','cfg')}",
         f"{'':10}{'gross':>10}{'net':>10}",
         f"{'excess':10}{g['excess_return']:>+10.4f}{n['excess_return']:>+10.4f}",
         f"{'sharpe':10}{'—':>10}{((n.get('risk') or {}).get('sharpe') or float('nan')):>10.2f}",
         f"{'maxDD':10}{g['max_drawdown']:>10.4f}{n['max_drawdown']:>10.4f}",
         f"turn/d {n['turnover']/max(n['n_rebalances'],1)/2*100:.1f}%   break-even {('%.0fbps'%m['break_even']) if m['break_even'] else 'N/A'}",
         f"regime net excess : train {f(m['net_train_ex'])} | OOS {f(m['net_oos_ex'])}"]
    if sweep is not None:
        L.append(f"tier2 sweep net-excess (top30/50/100 × reb1/5): {[round(x,3) for x in sweep]}  sign-flip={detect_sign_flip(sweep)}")
    L += [f"flags   : {flags or ['none']}",
          f"VERDICT : {verdict}",
          f"vs prior-best net-OOS : {f(prior)} → {f(m['net_oos_ex'])}",
          "=" * 56]
    return "\n".join(L)

def append_ledger(rnd, label, note, axis, verdict, flags, m):
    os.makedirs(os.path.dirname(LEDGER_JSONL), exist_ok=True)
    rec = {"round": rnd, "label": label, "axis": axis, "note": note,
           "verdict": verdict, "flags": flags, **m}
    with open(LEDGER_JSONL, "a", encoding="utf-8") as fp:
        fp.write(json.dumps(rec, ensure_ascii=False) + "\n")
    row = (f"| {rnd} | {label} | {note} | {m['net_ex']:+.3f} | "
           f"{(m['net_oos_ex'] if m['net_oos_ex'] is not None else float('nan')):+.3f} | "
           f"{(m['net_sharpe'] if m['net_sharpe'] is not None else float('nan')):.2f} | "
           f"{axis} | {','.join(flags) or '—'} | {verdict} |")
    with open(LEDGER_MD, "a", encoding="utf-8") as fp:
        fp.write(row + "\n")

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("config"); ap.add_argument("--note", required=True)
    ap.add_argument("--axis", default="daily", choices=list(AXES))
    ap.add_argument("--label", default=None); ap.add_argument("--top", type=int, default=50)
    a = ap.parse_args()
    label = a.label or os.path.splitext(os.path.basename(a.config))[0]
    rnd = _next_round(); prior = _prior_best_oos()
    g, n, ax = run(a.config, a.axis, a.top, label, a.note)
    v0, flags0, m = judge(g, n, sweep=None)        # Tier-1
    sweep = None
    if v0 == "PASS":                                # Tier-2 仅过门触发
        sweep = tier2_sweep(a.config, a.axis, label)
        v, flags, _ = judge(g, n, sweep)
    else:
        v, flags = v0, flags0
    print(card(rnd, label, a.axis, a.note, g, n, ax, v, flags, m, sweep, prior))
    append_ledger(rnd, label, a.note, a.axis, v, flags, m)

if __name__ == "__main__":
    main()
```

- [ ] **Step 2: 冒烟**（需先有 universe_baostock_day.csv + 一个 iter 配置；用 Task 4 的 value_pb）。先确认 import 与 argparse：`python scripts/iterate.py --help` → 显示 config/--note/--axis/--top。

- [ ] **Step 3: 提交**
```bash
git add scripts/iterate.py
git commit -m "feat(iterate): tiered run + round-card + ledger append"
```

---

### Task 3: 迭代账本种子 + /loop prompt 文档

**Files:** Create `docs/superpowers/iteration-ledger.md`；`.gitignore` 加 `.iter/`

- [ ] **Step 1:** 写 `docs/superpowers/iteration-ledger.md`：顶部放 /loop prompt 模板（spec §7 逐字）；"已证伪角度(勿重试)"区（种入 27 轮 + 7 日频 + 3 日内变体的全部证伪结论：反转/动量/低波/价值-超额/中度反转/value×低波/value池内反转/MACD/道氏/RSI/布林/Brooks/规模代理/价值+长趋势(OOS)/质量/成长/价值×动量；唯一稳健边=价值-防御慢调仓）；"待试角度"区（多因子AND组合、价值×质量双优、扩展TA组合、板块相对强弱[需引擎]、日内微结构[日内轴]）；运行表表头 `| round | label | 假设 | net超额 | net-OOS超额 | netSharpe | axis | flags | 裁决 |`。
- [ ] **Step 2:** `.gitignore` 追加 `.iter/`。
- [ ] **Step 3: 提交** `git add docs/superpowers/iteration-ledger.md .gitignore && git commit -m "docs(iterate): seed iteration ledger + /loop prompt + falsified-angle digest"`

---

### Task 4: e2e smoke（value_pb 基线一轮）+ 收尾

**Files:** Create `examples/screen/iter/value_pb_base.yaml`（quality=value_pb，setup=value_pb，λ=0，regimes=train 2018-2023 + OOS 2024-2026）

- [ ] **Step 1:** 写 `examples/screen/iter/value_pb_base.yaml`（复用 examples/trees/screen/value_pb.yaml；regimes: `{label: train, from: 2018-01-02, to: 2023-12-29}` + `{label: "2024-26_OOS", from: 2024-01-02, to: 2026-06-30}`）。
- [ ] **Step 2: 跑一轮** `python scripts/iterate.py examples/screen/iter/value_pb_base.yaml --note "baseline: 纯PB价值防御基线" --axis daily --top 50`
  Expected: 打印轮卡（含 train/OOS net excess、flags、VERDICT）；`.iter/ledger.jsonl` +1 行；ledger.md 表 +1 行。价值基线预期 FALSIFIED(超额)但 train>0/OOS lag 合理、净Sharpe 正。
- [ ] **Step 3: 校验账本** `tail -1 .iter/ledger.jsonl` 字段完整；`tail -2 docs/superpowers/iteration-ledger.md` 有表行。
- [ ] **Step 4: 提交** `git add examples/screen/iter/value_pb_base.yaml docs/superpowers/iteration-ledger.md && git commit -m "test(iterate): e2e smoke value_pb baseline round + ledger entry"`

---

## Self-Review
- **Spec 覆盖**：iterate.py(Task1/2)✓ 分层+旗标+裁决+轮卡+账本；账本(Task3)✓ md+jsonl+种子+queue；/loop prompt(Task3)✓；回测口径(Task2 AXES)✓ daily/intraday;测试(Task1 单测 + Task4 e2e)✓；引擎缺口(板块)在 spec/queue 标注✓。
- **占位符**：无（纯函数全代码；I/O 全代码；smoke 命令具体）。
- **类型一致**：judge/break_even/regime_excess/detect_sign_flip 签名跨 Task1↔2 一致；daily_eval.run_once 签名与现有一致(cfg,cost,frm,to,warmup,window,top,out,universe,membership)。
- **gap**：universe_intraday_day.csv（日内轴）尚不存在——日内轴留到首次用时再 build（daily 轴为主，不阻塞 smoke）。已在 queue 标注。
