#!/usr/bin/env python3
"""生成 12 个日内选股树 + 12 配置（6 因子 × hi/lo）。codegen，可复现。

树：信号作 quality（weight=sigmoid(±scale·fund.F)），lambda=0 → combined=quality → select_top 直选。
sigmoid 单调 → 排名与 scale 无关；fund.F=NaN → 权重 clamp 0 → 被 select_top 排除（弃权）。
配置 regimes = 时间二分（前半/后半_OOS）作 6mo 单 regime 下唯一弱稳健检查。
"""
import os
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TREES = os.path.join(REPO, "examples", "trees", "screen")
CFGS = os.path.join(REPO, "examples", "screen")

FACTORS = {
    "last_leg":       "尾盘动量",
    "intraday_rev":   "日内反转",
    "close_vs_vwap":  "收盘强度",
    "intraday_range": "日内波幅",
    "vol_tilt":       "量能后移",
    "overnight":      "隔夜跳空",
}
DIRS = {"hi": "scale * fund.{f}", "lo": "0 - scale * fund.{f}"}

TREE_TMPL = """# 日内因子 {cn} {dirn}：信号作 quality，select_top 直选（lambda=0）。fund.{f}=NaN→弃权。
meta: {{ name: intraday_{f}_{d}, forward_window: 1, stances: [long, flat] }}
params: {{ scale: 1.0 }}
root: gate
nodes:
  gate:
    type: quant
    branches:
      - {{ when: "close > 0", goto: pick, label: ok }}
    default: {{ goto: flat, label: flat }}
leaves:
  pick: {{ stance: long, weight: "sigmoid({expr})" }}
  flat: {{ stance: flat }}
"""

CFG_TMPL = """# 日频日内选股：{cn} {dirn}，每日尾盘选 fund.{f} {sel} top-50，持有1日。
quality_trees: [examples/trees/screen/intraday_{f}_{d}.yaml]
setup_trees:
  日内: [examples/trees/screen/intraday_{f}_{d}.yaml]
merge: {{ q_floor: 0.0, top: 50, lambda: 0.0, tilt_setups: ["日内"], quality_layers: 5 }}
regimes:
  - {{ label: "前半", from: 2025-12-10, to: 2026-03-15 }}
  - {{ label: "后半_OOS", from: 2026-03-16, to: 2026-06-16 }}
"""

DIRN = {"hi": "高(选最高)", "lo": "低(选最低)"}
SEL = {"hi": "最高", "lo": "最低"}

n = 0
for f, cn in FACTORS.items():
    for d, exprt in DIRS.items():
        expr = exprt.format(f=f)
        with open(os.path.join(TREES, f"intraday_{f}_{d}.yaml"), "w", encoding="utf-8") as fh:
            fh.write(TREE_TMPL.format(cn=cn, dirn=DIRN[d], f=f, d=d, expr=expr))
        with open(os.path.join(CFGS, f"daily_intraday_{f}_{d}.yaml"), "w", encoding="utf-8") as fh:
            fh.write(CFG_TMPL.format(cn=cn, dirn=DIRN[d], f=f, d=d, sel=SEL[d]))
        n += 2
print(f"generated {n} files ({len(FACTORS)} factors × 2 dirs × (tree+config))")
