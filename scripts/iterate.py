#!/usr/bin/env python3
"""Claude-in-the-loop 选股树迭代轮驱动。

见 docs/superpowers/specs/2026-06-18-iteration-harness-design.md。
分层回测(Tier-1 gross/net + train/OOS；过门才 Tier-2 敏感性) + 过拟合自动旗标 + 裁决
+ 账本追加 + 轮卡打印。脚本只执行+记录+护栏，不改树/不调参凑数(§5.3)。
"""
import argparse, json, os, sys, time
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
