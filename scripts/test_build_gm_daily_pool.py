#!/usr/bin/env python3
"""build_gm_daily_pool 纯函数单测：指标 + 门槛 + 财务门槛 + 粗排取顶。

跑：python -m pytest scripts/test_build_gm_daily_pool.py -q
或：python scripts/test_build_gm_daily_pool.py
"""
import math

from build_gm_daily_pool import (to_float, daily_metrics, passes_gates,
                                  fund_ok, score, select_top)


def test_to_float():
    assert to_float("1.5") == 1.5
    assert to_float("") is None and to_float(None) is None and to_float("x") is None


def test_daily_metrics_basic():
    closes = [10.0, 11.0, 12.0]
    amounts = [1e8, 2e8, 3e8]
    turns = [1.0, 2.0, 3.0]
    m = daily_metrics(closes, amounts, turns, window=3)
    assert m["price"] == 12.0
    assert math.isclose(m["avg_amount"], 2e8)
    assert math.isclose(m["mom"], 12.0 / 10.0 - 1)  # window 日收益
    assert math.isclose(m["avg_turn"], 2.0)
    assert m["susp_recent"] == 0 and m["n_bars"] == 3


def test_daily_metrics_short_and_susp():
    assert daily_metrics([10.0], [1e8], [1.0], window=3) is None   # 历史不足
    m = daily_metrics([10, 11, 12], [1e8, 0, 3e8], [1, None, 3], window=3)
    assert m["susp_recent"] == 1                                   # amount=0 计停牌
    assert math.isclose(m["avg_amount"], (1e8 + 3e8) / 2)          # 停牌日(0)不计入均额
    # 末值非法 → None
    assert daily_metrics([10, 11, 0], [1e8, 1e8, 1e8], [1, 1, 1], window=3) is None


def _m(**kw):
    base = {"price": 10.0, "avg_amount": 1e8, "mom": 0.05, "avg_turn": 2.0,
            "n_bars": 300, "susp_recent": 0}
    base.update(kw)
    return base


def test_passes_gates():
    assert passes_gates(_m(), 2.0, 0.0, 5e7, 3)
    assert not passes_gates(None, 2.0, 0.0, 5e7, 3)
    assert not passes_gates(_m(price=1.5), 2.0, 0.0, 5e7, 3)        # 仙股
    assert not passes_gates(_m(price=50), 2.0, 30.0, 5e7, 3)        # 超上限
    assert not passes_gates(_m(avg_amount=1e7), 2.0, 0.0, 5e7, 3)   # 流动性
    assert not passes_gates(_m(susp_recent=5), 2.0, 0.0, 5e7, 3)    # 停牌多


def test_fund_ok():
    assert fund_ok(8.0, 0.2, None, None)                # 门槛全关 → 过
    assert fund_ok(8.0, 0.2, 5.0, 0.0)                  # 达标
    assert not fund_ok(3.0, 0.2, 5.0, None)             # roe 不足
    assert not fund_ok(None, 0.2, 5.0, None)            # 开门槛但缺数据 → 剔
    assert not fund_ok(8.0, -0.1, None, 0.0)            # 净利同比为负


def test_score_keys():
    m = _m(avg_amount=3e8, mom=0.1, avg_turn=4.0)
    assert score(m, "liquidity") == 3e8
    assert score(m, "momentum") == 0.1
    assert score(m, "turnover") == 4.0


def test_select_top():
    items = [("a", _m(avg_amount=1e8)), ("b", _m(avg_amount=3e8)), ("c", _m(avg_amount=2e8))]
    assert select_top(items, "liquidity", 2) == ["b", "c"]
    assert select_top(items, "liquidity", 0) == ["b", "c", "a"]
    # 不可算分(mom=None)→剔除
    items2 = [("a", _m(mom=0.1)), ("b", _m(mom=None))]
    assert select_top(items2, "momentum", 0) == ["a"]


if __name__ == "__main__":
    for name, fn in sorted(globals().items()):
        if name.startswith("test_") and callable(fn):
            fn(); print(f"PASS {name}")
    print("all passed")
