#!/usr/bin/env python3
"""build_gm_shortlist 纯函数单测：解析 + 各门槛 + 各粗排键 + 取 top。

跑：python -m pytest scripts/test_build_gm_shortlist.py -q
或：python scripts/test_build_gm_shortlist.py
"""
import math

from build_gm_shortlist import to_float, parse_row, passes_gates, score, select_top


def test_to_float():
    assert to_float("1.5") == 1.5
    assert to_float("") is None
    assert to_float(None) is None
    assert to_float("abc") is None


def test_parse_row():
    r = parse_row({"symbol": "sh600519", "open": "1235.0", "high": "1238.87",
                   "low": "1211.22", "price": "1215.0", "cum_volume": "5747173",
                   "cum_amount": "7016713941.0", "ask1": "1215.28", "ask1_v": "100"})
    assert r["symbol"] == "sh600519" and r["price"] == 1215.0
    assert r["cum_amount"] == 7016713941.0 and r["ask1_v"] == 100.0


def _row(**kw):
    base = {"symbol": "sh600000", "open": 10.0, "high": 10.5, "low": 9.8, "price": 10.2,
            "cum_volume": 1e6, "cum_amount": 1e8, "bid1": 10.19, "bid1_v": 100,
            "ask1": 10.2, "ask1_v": 100}
    base.update(kw)
    return base


def test_gate_suspended_and_liquidity_and_price():
    assert passes_gates(_row(), 2.0, 0.0, 3e7, False)
    assert not passes_gates(_row(price=None), 2.0, 0.0, 3e7, False)      # 停牌
    assert not passes_gates(_row(cum_volume=0), 2.0, 0.0, 3e7, False)    # 零成交
    assert not passes_gates(_row(price=1.5), 2.0, 0.0, 3e7, False)       # 仙股
    assert not passes_gates(_row(cum_amount=1e6), 2.0, 0.0, 3e7, False)  # 流动性不足
    assert not passes_gates(_row(price=50), 2.0, 30.0, 3e7, False)       # 超价上限


def test_gate_drop_limit_up():
    """无卖盘(ask1_v=0/None) + 开关开 → 视为涨停封板剔除。"""
    assert passes_gates(_row(ask1_v=0), 2.0, 0.0, 3e7, False)            # 开关关 → 不剔
    assert not passes_gates(_row(ask1_v=0), 2.0, 0.0, 3e7, True)         # 开关开 → 剔
    assert not passes_gates(_row(ask1=None, ask1_v=None), 2.0, 0.0, 3e7, True)


def test_score_keys():
    r = _row(open=10.0, price=10.2, high=10.5, low=9.8, cum_volume=1000, cum_amount=10100)
    assert score(r, "liquidity") == 10100
    assert math.isclose(score(r, "intraday"), 10.2 / 10.0 - 1)
    assert math.isclose(score(r, "range_pos"), (10.2 - 9.8) / (10.5 - 9.8))
    assert math.isclose(score(r, "vwap_gap"), 10.2 / (10100 / 1000) - 1)
    assert score(_row(open=0), "intraday") is None      # 不可算 → None
    assert score(_row(high=5, low=5), "range_pos") is None


def test_select_top_sorts_desc_and_caps():
    rows = [_row(symbol="a", cum_amount=1e8), _row(symbol="b", cum_amount=3e8),
            _row(symbol="c", cum_amount=2e8)]
    picked = select_top(rows, "liquidity", 2)
    assert [r["symbol"] for r in picked] == ["b", "c"]   # 降序取前2
    assert len(select_top(rows, "liquidity", 0)) == 3    # 0=全部


def test_select_top_drops_uncomputable():
    rows = [_row(symbol="a", open=10, price=11), _row(symbol="b", open=0, price=11)]
    picked = select_top(rows, "intraday", 0)             # b 的 open=0 → 分 None → 排除
    assert [r["symbol"] for r in picked] == ["a"]


if __name__ == "__main__":
    for name, fn in sorted(globals().items()):
        if name.startswith("test_") and callable(fn):
            fn(); print(f"PASS {name}")
    print("all passed")
