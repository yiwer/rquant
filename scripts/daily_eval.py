#!/usr/bin/env python3
"""日频选股评估框架 (daily fast-in/out, EOD rebalance, selection-focused).

对一个 screen 配置以 --rebalance 1（尾盘日调、持有1日）跑两次回测——
gross(cost=0) 与 net(cost=COST bps)——再把日频策略真正该看的诊断量提到台前：

  · 日均换手 / 单边换手% / 年化换手倍数   ← 日频成本墙的来源
  · 毛 vs 净 的总收益 / 超额 / Sharpe / 回撤
  · gross→net alpha 衰减                  ← 成本吃掉多少
  · break-even cost (bps)                 ← 净超额归零的成本（仅毛超额>0时有意义）
  · 各 regime 的毛/净超额                  ← OOS 是金标准
  · 诚实裁决：净 OOS 超额>0 且 净 Sharpe>0 才算"未证伪"

用法:
  python scripts/daily_eval.py <config.yaml> [--cost 20] [--from 2018-01-01] [--to 2026-06-12]
                               [--warmup 10] [--window 60] [--top N] [--label NAME]
                               [--oos-label 2023-25_OOS]

诚实文化：证伪是合法且有价值的产出；本工具刻意把净指标与换手放在最显眼处，绝不为好看数字调参。
"""
import argparse, json, os, subprocess, sys

try:
    sys.stdout.reconfigure(encoding="utf-8")  # Windows 控制台默认 GBK，强制 UTF-8 防中文乱码
except Exception:
    pass

WT = "E:/rust-app/rquant/.claude/worktrees/worktree-treeloop2"
BIN = "E:/rust-app/rquant/target/release/rquant.exe"
UNIV = "E:/rust-app/rquant/data/universe_membership.csv"
MEMB = "E:/rust-app/rquant/data/membership_top2000.csv"
RUNS = os.path.join(WT, ".daily_runs")
TRADING_DAYS = 242  # A股年化交易日近似


def run_once(cfg, cost, frm, to, warmup, window, top, out):
    cmd = [BIN, "screen", "--backtest", "--universe", UNIV, "--config", cfg,
           "--membership", MEMB, "--rebalance", "1", "--warmup", str(warmup),
           "--window", str(window), "--cost-bps", str(cost),
           "--from", frm, "--to", to, "--out", out]
    if top is not None:
        cmd += ["--top", str(top)]
    # 不捕获 binary 的 stdout（其中文摘要在 Windows 会触发 GBK 解码崩溃）；只需 JSON 文件。
    r = subprocess.run(cmd, cwd=WT, stdout=subprocess.DEVNULL,
                       stderr=subprocess.PIPE, encoding="utf-8", errors="replace")
    if not os.path.exists(out):
        sys.stderr.write((r.stderr or "") + "\n")
        raise SystemExit(f"run failed (cost={cost}): no output at {out}")
    with open(out, encoding="utf-8") as f:
        return json.load(f)


def regime_map(rep):
    return {s["label"]: s for s in rep.get("regime_slices", [])}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("config")
    ap.add_argument("--cost", type=float, default=20.0)
    ap.add_argument("--from", dest="frm", default="2018-01-01")
    ap.add_argument("--to", default="2026-06-12")
    ap.add_argument("--warmup", type=int, default=10)
    ap.add_argument("--window", type=int, default=60)
    ap.add_argument("--top", type=int, default=None)
    ap.add_argument("--label", default=None)
    ap.add_argument("--oos-label", default="2023-25_OOS")
    a = ap.parse_args()

    label = a.label or os.path.splitext(os.path.basename(a.config))[0]
    os.makedirs(RUNS, exist_ok=True)
    g_out = os.path.join(RUNS, f"{label}_gross.json")
    n_out = os.path.join(RUNS, f"{label}_net{int(a.cost)}.json")

    g = run_once(a.config, 0.0, a.frm, a.to, a.warmup, a.window, a.top, g_out)
    n = run_once(a.config, a.cost, a.frm, a.to, a.warmup, a.window, a.top, n_out)

    nreb = g["n_rebalances"]
    tov = g["turnover"]                       # Σ|Δw| 累计；与成本无关（两次相同）
    per = tov / nreb if nreb else 0.0         # 每调仓 Σ|Δw|（满仓全换=2.0）
    one_side = per / 2.0                       # 单边换手比例（0.5=每日替换一半）
    ann_tov = one_side * TRADING_DAYS         # 年化单边换手倍数

    g_ex, n_ex = g["excess_return"], n["excess_return"]
    g_sh = (g.get("risk") or {}).get("sharpe")
    n_sh = (n.get("risk") or {}).get("sharpe")
    decay = g_ex - n_ex                        # 成本吃掉的超额（@cost bps）
    be = a.cost * g_ex / decay if decay > 0 and g_ex > 0 else None

    def f(x): return "—" if x is None else f"{x:+.4f}"
    def fs(x): return "—" if x is None else f"{x:.2f}"

    print(f"\n========== 日频选股评估 · {label} (top={a.top or 'cfg'}, cost={a.cost:.0f}bps) ==========")
    print(f"区间 {a.frm}~{a.to}   调仓次数 {nreb}")
    print(f"换手   每调仓Σ|Δw| {per:.3f}   单边日换手 {one_side*100:.1f}%   年化单边换手 {ann_tov:.0f}x")
    print(f"{'':8}{'毛(0bps)':>12}{'净('+str(int(a.cost))+'bps)':>14}")
    print(f"{'总收益':8}{g['total_return']:>+12.4f}{n['total_return']:>+14.4f}")
    print(f"{'基准':8}{g['benchmark_return']:>+12.4f}{n['benchmark_return']:>+14.4f}")
    print(f"{'超额':8}{g_ex:>+12.4f}{n_ex:>+14.4f}")
    print(f"{'Sharpe':8}{fs(g_sh):>12}{fs(n_sh):>14}")
    print(f"{'最大回撤':8}{g['max_drawdown']:>12.4f}{n['max_drawdown']:>14.4f}")
    print(f"成本衰减(超额) -{decay:.4f}   break-even成本 {('%.1fbps'%be) if be else 'N/A(毛超额≤0)'}")

    print("--- 各 regime 超额 (毛 / 净) ---")
    gm, nm = regime_map(g), regime_map(n)
    for lab in [s["label"] for s in g.get("regime_slices", [])]:
        ge = gm[lab]["excess"]; ne = nm[lab]["excess"]
        print(f"  {lab:14} 毛 {ge:+.4f}   净 {ne:+.4f}")

    # ---- 诚实裁决 ----
    oos = nm.get(a.oos_label)
    oos_ex = oos["excess"] if oos else None
    reasons = []
    if g_ex <= 0:
        reasons.append(f"毛超额≤0({g_ex:+.3f})→源头无 alpha")
    if oos_ex is not None and oos_ex <= 0:
        reasons.append(f"净 OOS 超额≤0({oos_ex:+.3f})")
    if n_sh is not None and n_sh <= 0:
        reasons.append(f"净 Sharpe≤0({n_sh:.2f})")
    verdict = "未证伪 (PASS)" if not reasons else "证伪 (FALSIFIED): " + "；".join(reasons)
    print(f"裁决: {verdict}")
    print("=" * 64)


if __name__ == "__main__":
    main()
