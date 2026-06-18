#!/usr/bin/env python3
"""Claude-in-the-loop 选股树迭代轮驱动。

见 docs/superpowers/specs/2026-06-18-iteration-harness-design.md。
分层回测(Tier-1 gross/net + train/OOS；过门才 Tier-2 敏感性) + 过拟合自动旗标 + 裁决
+ 账本追加 + 轮卡打印。脚本只执行+记录+护栏，不改树/不调参凑数(§5.3)。
"""
import argparse, bisect, csv, json, os, sys, time
try:
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:
    pass
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import daily_eval as de  # 复用 run_once / REPO_ROOT / RUNS / BIN

COST = 20.0       # 净成本 bps
BE_MIN = 40.0     # break-even 门槛 = 2×成本
OOS_TAG = "OOS"   # regime 标签含此 = 样本外窗


def break_even(gross_ex, net_ex, cost):
    """净超额归零的成本 bps；仅毛超额>0 且有衰减时有意义。"""
    decay = gross_ex - net_ex
    return cost * gross_ex / decay if (decay > 0 and gross_ex > 0) else None


def regime_excess(report, oos):
    """取 regime 切片净超额：oos=True 取标签含 OOS 者，False 取首个非 OOS。"""
    for s in report.get("regime_slices", []):
        if (OOS_TAG in s["label"]) == oos:
            return s["excess"]
    return None


def detect_sign_flip(net_excesses):
    """参数扫描里净超额既有正又有负 = 非稳健。"""
    xs = [x for x in net_excesses if x is not None]
    return any(x > 0 for x in xs) and any(x < 0 for x in xs)


def judge(g, n, sweep):
    """g/n=gross/net 报告 dict；sweep=参数扫描净超额列表或 None。
    返回 (verdict, flags, metrics)。PASS 需全满足 §5.3 门槛。"""
    gx, nx = g["excess_return"], n["excess_return"]
    nsh = (n.get("risk") or {}).get("sharpe")
    oos, tr = regime_excess(n, True), regime_excess(n, False)
    be = break_even(gx, nx, COST)
    flags = []
    if gx <= 0:
        flags.append("gross-excess<=0")
    if oos is not None and oos <= 0:
        flags.append("net-OOS<=0")
    if nsh is not None and nsh <= 0:
        flags.append("net-sharpe<=0")
    if tr is not None and oos is not None and tr > 0 >= oos:
        flags.append("in-sample-only")
    if be is None or be < BE_MIN:
        flags.append(f"break-even<{int(BE_MIN)}bps")
    if sweep is not None and detect_sign_flip(sweep):
        flags.append("sign-flip")
    passed = (gx > 0 and oos is not None and oos > 0 and nsh is not None and nsh > 0
              and be is not None and be >= BE_MIN
              and (sweep is None or not detect_sign_flip(sweep)))
    metrics = {"gross_ex": gx, "net_ex": nx, "net_oos_ex": oos, "net_train_ex": tr,
               "net_sharpe": nsh, "break_even": be}
    return ("PASS" if passed else "FALSIFIED"), flags, metrics


# ---------------------------------------------------------------------------
# 换框架：对可交易宽基指数重算超额（vs-EW → vs-index），剥离等权小盘 beta
# ---------------------------------------------------------------------------
INDEX_DIR = os.path.join(de.REPO_ROOT, "data", "baostock", "index")


def load_index(name):
    """读 data/baostock/index/<name>.csv（time,close）→ ({date: close}, sorted_dates)。"""
    m = {}
    with open(os.path.join(INDEX_DIR, f"{name}.csv"), encoding="utf-8") as f:
        for row in csv.DictReader(f):
            m[row["time"][:10]] = float(row["close"])
    return m, sorted(m)


def _idx_at(m, dates, d):
    """指数在日期 d 的收盘（取 ≤d 最近交易日）。"""
    i = bisect.bisect_right(dates, d) - 1
    return m[dates[i]] if i >= 0 else None


def to_index_relative(report, m, dates):
    """从 screen 报告的 nav 曲线(holdings) + 指数序列重算超额(vs index)。
    返回 report-like dict（excess_return/risk/regime_slices/total_return…）供 judge 复用。
    risk(绝对 Sharpe) 原样透传；超额改 vs 指数。regime 窗口取自报告 regime_slices 的 from/to。"""
    nav = [(h["t"][:10], h["nav"]) for h in report.get("holdings", []) if h.get("nav", 0) > 0]
    if len(nav) < 2:
        return None
    strat_total = nav[-1][1] / nav[0][1] - 1.0
    i0, iN = _idx_at(m, dates, nav[0][0]), _idx_at(m, dates, nav[-1][0])
    idx_total = (iN / i0 - 1.0) if (i0 and iN) else None
    excess = (strat_total - idx_total) if idx_total is not None else None
    slices = []
    for s in report.get("regime_slices", []):
        fr, to = s.get("from"), s.get("to")
        sub = [(d, v) for d, v in nav if fr <= d <= to]
        if len(sub) >= 2:
            sr = sub[-1][1] / sub[0][1] - 1.0
            x0, x1 = _idx_at(m, dates, sub[0][0]), _idx_at(m, dates, sub[-1][0])
            ex = (sr - (x1 / x0 - 1.0)) if (x0 and x1) else None
            slices.append({"label": s["label"], "excess": ex})
    return {"excess_return": excess, "risk": report.get("risk"), "regime_slices": slices,
            "total_return": strat_total, "max_drawdown": report["max_drawdown"],
            "turnover": report["turnover"], "n_rebalances": report["n_rebalances"]}


# ---------------------------------------------------------------------------
# I/O 层：跑回测(Tier-1/2) + 轮卡 + 账本追加
# ---------------------------------------------------------------------------
LEDGER_MD = os.path.join(de.REPO_ROOT, "docs", "superpowers", "iteration-ledger.md")
LEDGER_JSONL = os.path.join(de.REPO_ROOT, ".iter", "ledger.jsonl")
TRADING_DAYS = 242

# 回测轴：universe + 默认窗口（spec §6）。OOS 标签必须含 "OOS"。日线为主轴。
AXES = {
    "daily": {"universe": "data/baostock/universe_baostock_day.csv",
              "frm": "2018-01-01", "to": "2026-06-12", "warmup": 60, "window": 60,
              "regimes_hint": "train 2018-01..2023-12 / OOS 2024-01..2026-06"},
    "intraday": {"universe": "data/baostock/universe_intraday_day.csv",
                 "frm": "2021-01-01", "to": "2026-06-12", "warmup": 5, "window": 10,
                 "regimes_hint": "train 2021..2023 / OOS 2024..2026"},
}


def _next_round():
    if not os.path.exists(LEDGER_JSONL):
        return 1
    with open(LEDGER_JSONL, encoding="utf-8") as f:
        return sum(1 for _ in f) + 1


def _prior_best_oos():
    best = None
    if os.path.exists(LEDGER_JSONL):
        with open(LEDGER_JSONL, encoding="utf-8") as f:
            for line in f:
                try:
                    o = json.loads(line).get("net_oos_ex")
                except Exception:
                    continue
                if o is not None and (best is None or o > best):
                    best = o
    return best


def run(cfg, axis, top, label, reb=1, sectors=None):
    """Tier-1：gross(0) + net(COST)。reb=1 且无 sectors 复用 run_once；否则走 _screen（部署节奏/行业中性）。"""
    a = AXES[axis]
    uni = os.path.join(de.REPO_ROOT, a["universe"])
    os.makedirs(de.RUNS, exist_ok=True)
    g_out = os.path.join(de.RUNS, f"iter_{label}_gross.json")
    n_out = os.path.join(de.RUNS, f"iter_{label}_net.json")
    if reb == 1 and not sectors:
        g = de.run_once(cfg, 0.0, a["frm"], a["to"], a["warmup"], a["window"], top, g_out, uni, "none")
        n = de.run_once(cfg, COST, a["frm"], a["to"], a["warmup"], a["window"], top, n_out, uni, "none")
    else:
        g = _screen(cfg, 0.0, a, top, reb, g_out, uni, sectors)
        n = _screen(cfg, COST, a, top, reb, n_out, uni, sectors)
    return g, n, a


def _screen(cfg, cost, a, top, reb, out, uni, sectors=None):
    """直接调引擎（支持 --rebalance 扫描 + --sectors 行业中性，run_once 不支持故不可复用）。"""
    import subprocess
    cmd = [de.BIN, "screen", "--backtest", "--universe", uni, "--config", cfg,
           "--rebalance", str(reb), "--warmup", str(a["warmup"]), "--window", str(a["window"]),
           "--cost-bps", str(cost), "--from", a["frm"], "--to", a["to"], "--top", str(top), "--out", out]
    if sectors:
        cmd += ["--sectors", sectors]
    subprocess.run(cmd, cwd=de.REPO_ROOT, stdout=subprocess.DEVNULL,
                   stderr=subprocess.PIPE, encoding="utf-8", errors="replace")
    with open(out, encoding="utf-8") as f:
        return json.load(f)


def tier2_sweep(cfg, axis, label, bench=None, sectors=None):
    """敏感性：top∈{30,50,100} × reb∈{1,5} 各跑 net，收集净超额（检符号翻转）。
    bench 给定时超额改 vs 指数（与主裁决同口径）；sectors 给定时行业中性(per_sector 来自配置)。"""
    a = AXES[axis]
    uni = os.path.join(de.REPO_ROOT, a["universe"])
    idx = load_index(bench) if bench else None
    out = []
    for top in (30, 50, 100):
        for reb in (1, 5):
            o = os.path.join(de.RUNS, f"iter_{label}_s{top}_{reb}.json")
            rep = _screen(cfg, COST, a, top, reb, o, uni, sectors)
            if idx:
                ir = to_index_relative(rep, *idx)
                out.append(ir["excess_return"] if ir else None)
            else:
                out.append(rep["excess_return"])
    return out


def card(rnd, label, axis, note, top, g, n, a, verdict, flags, m, sweep, prior, bench=None, ew=None, reb=1):
    def f(x):
        return "—" if x is None else f"{x:+.4f}"

    def fs(x):
        return "—" if x is None else f"{x:.2f}"

    nsh = (n.get("risk") or {}).get("sharpe")
    gsh = (g.get("risk") or {}).get("sharpe")
    nreb = max(n.get("n_rebalances", 0), 1)
    one_side = n.get("turnover", 0.0) / nreb / 2.0
    bench_lbl = f"index:{bench}" if bench else "universe-EW"
    L = [f"=== ITER round {rnd} · {label} (axis={axis}) ===",
         f"hypothesis : {note}",
         f"universe   : {os.path.basename(a['universe'])}   {a['regimes_hint']}   top {top}  reb {reb}  cost {int(COST)}bps",
         f"benchmark  : {bench_lbl}" + (f"   (excess below is vs {bench}; turnover/maxDD/sharpe are strategy-absolute)" if bench else ""),
         f"{'':10}{'gross':>11}{'net':>11}",
         f"{'total':10}{g['total_return']:>+11.4f}{n['total_return']:>+11.4f}",
         f"{'excess':10}{g['excess_return']:>+11.4f}{n['excess_return']:>+11.4f}",
         f"{'sharpe':10}{fs(gsh):>11}{fs(nsh):>11}",
         f"{'maxDD':10}{g['max_drawdown']:>11.4f}{n['max_drawdown']:>11.4f}",
         f"turnover/d {one_side*100:.1f}%   break-even {('%.0fbps' % m['break_even']) if m['break_even'] else 'N/A(gross<=0)'}",
         f"regime net excess : train {f(m['net_train_ex'])} | OOS {f(m['net_oos_ex'])}"]
    if ew is not None:
        L.append(f"(ref) vs-EW excess : gross {f(ew[0])} | net {f(ew[1])}")
    if sweep is not None:
        L.append(f"tier2 net-excess (top30/50/100 x reb1/5): {[round(x, 3) for x in sweep]}  sign-flip={detect_sign_flip(sweep)}")
    L += [f"flags   : {flags or ['none']}",
          f"VERDICT : {verdict}",
          f"vs prior-best net-OOS : {f(prior)} -> {f(m['net_oos_ex'])}",
          "=" * 56]
    return "\n".join(L)


def write_round_sidecar(iter_dir, rnd, label, bench, reb, config_path, tier2_cells):
    """写 .iter/round_<rnd>.json 供 GUI round card 读取(tier2 cells + 配置路径)。纯持久化,不影响裁决。"""
    import json, os
    os.makedirs(iter_dir, exist_ok=True)
    path = os.path.join(iter_dir, f"round_{rnd}.json")
    rec = {"round": rnd, "label": label, "benchmark": bench or "EW", "rebalance": reb,
           "config_path": config_path, "tier2": tier2_cells}
    with open(path, "w", encoding="utf-8") as fp:
        json.dump(rec, fp, ensure_ascii=False, indent=2)
    return path


def append_ledger(rnd, label, note, axis, verdict, flags, m, bench=None, reb=1):
    os.makedirs(os.path.dirname(LEDGER_JSONL), exist_ok=True)
    rec = {"round": rnd, "label": label, "axis": axis, "note": note,
           "benchmark": bench or "EW", "rebalance": reb, "verdict": verdict, "flags": flags, **m}
    with open(LEDGER_JSONL, "a", encoding="utf-8") as fp:
        fp.write(json.dumps(rec, ensure_ascii=False) + "\n")
    oos = m["net_oos_ex"]
    sh = m["net_sharpe"]
    row = (f"| {rnd} | {label} | {note} | {m['net_ex']:+.3f} | "
           f"{('%.3f' % oos) if oos is not None else 'NA'} | "
           f"{('%.2f' % sh) if sh is not None else 'NA'} | "
           f"{axis} | {','.join(flags) or '—'} | {verdict} |")
    with open(LEDGER_MD, "a", encoding="utf-8") as fp:
        fp.write(row + "\n")


def main():
    ap = argparse.ArgumentParser(description="Claude-in-the-loop 选股树迭代轮驱动")
    ap.add_argument("config")
    ap.add_argument("--note", required=True, help="本轮假设（一句话）")
    ap.add_argument("--axis", default="daily", choices=list(AXES))
    ap.add_argument("--label", default=None)
    ap.add_argument("--top", type=int, default=50)
    ap.add_argument("--benchmark", default=None, choices=["csi300", "csi500", "csi1000"],
                    help="换框架：对可交易宽基指数算超额(剥离等权小盘 beta)；缺省=universe 等权")
    ap.add_argument("--rebalance", type=int, default=1, help="调仓间隔(bar)；1=日频(默认)，5≈周，20≈月(部署节奏)")
    ap.add_argument("--sectors", default=None, help="行业中性：symbol→行业 CSV(配 config merge.per_sector=k)")
    a = ap.parse_args()
    label = a.label or os.path.splitext(os.path.basename(a.config))[0]
    sectors = os.path.join(de.REPO_ROOT, a.sectors) if (a.sectors and not os.path.isabs(a.sectors)) else a.sectors
    rnd = _next_round()
    prior = _prior_best_oos()
    g, n, ax = run(a.config, a.axis, a.top, label, a.rebalance, sectors)
    bench = a.benchmark
    ew = None
    if bench:                                           # 换框架：超额改 vs 指数
        idx = load_index(bench)
        ew = (g["excess_return"], n["excess_return"])   # 透明：保留 vs-EW 供参考
        gi, ni = to_index_relative(g, *idx), to_index_relative(n, *idx)
        if gi is None or ni is None:
            raise SystemExit("index-relative conversion failed (nav curve too short)")
        g, n = gi, ni
    v0, flags0, m = judge(g, n, sweep=None)            # Tier-1
    sweep = None
    if v0 == "PASS":                                    # Tier-2 仅过 OOS 门触发
        sweep = tier2_sweep(a.config, a.axis, label, bench, sectors)
        v, flags, _ = judge(g, n, sweep)
    else:
        v, flags = v0, flags0
    note_led = (a.note + (f" [bench:{bench}]" if bench else "")
                + (f" [reb{a.rebalance}]" if a.rebalance != 1 else "")
                + (" [sector-neutral]" if sectors else ""))
    print(card(rnd, label, a.axis, a.note, a.top, g, n, ax, v, flags, m, sweep, prior, bench, ew, a.rebalance))
    append_ledger(rnd, label, note_led, a.axis, v, flags, m, bench, a.rebalance)
    try:
        _TIER2_GRID = [(t, r) for t in (30, 50, 100) for r in (1, 5)]
        tier2_cells = ([{"top": t, "rebalance": r, "net_excess": x}
                        for (t, r), x in zip(_TIER2_GRID, sweep)]
                       if sweep is not None else [])
        write_round_sidecar(os.path.dirname(LEDGER_JSONL), rnd, label, bench,
                            a.rebalance, a.config, tier2_cells)
    except Exception as _e:
        print(f"[warn] sidecar write failed (non-fatal): {_e}", file=sys.stderr)


if __name__ == "__main__":
    main()
