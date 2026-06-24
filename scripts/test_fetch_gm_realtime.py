#!/usr/bin/env python3
"""fetch_gm_realtime 纯函数单测：代码格式转换 + bar→行 + 最新节筛选 + CSV。

跑：python -m pytest scripts/test_fetch_gm_realtime.py -q
或：python scripts/test_fetch_gm_realtime.py
不联网、不需 token（gm 仅在 main() 内导入）。
"""
import datetime as dt
import os
import tempfile

from fetch_gm_realtime import (to_gm, to_local, bar_time, bars_to_rows, write_csv,
                               tick_to_row, write_rows, SNAP_COLS)


def test_to_gm():
    assert to_gm("sh600519") == "SHSE.600519"
    assert to_gm("sz300750") == "SZSE.300750"
    assert to_gm("600000.SH") == "SHSE.600000"
    assert to_gm("SHSE.600000") == "SHSE.600000"   # 已是掘金格式 → 原样
    assert to_gm("830799") == "BJSE.830799"
    assert to_gm("000001") == "SZSE.000001"        # 无前缀按首位推断
    assert to_gm("600519") == "SHSE.600519"


def test_to_local():
    assert to_local("SHSE.600519") == "sh600519"
    assert to_local("sz300750") == "sz300750"
    assert to_local("830799") == "bj830799"


def test_bar_time():
    assert bar_time(dt.datetime(2026, 6, 18, 15, 0, 0)) == "2026-06-18 15:00:00"


def _bar(eob_str, o, h, l, c, v, amt):
    return {"eob": dt.datetime.strptime(eob_str, "%Y-%m-%d %H:%M:%S"),
            "open": o, "high": h, "low": l, "close": c, "volume": v, "amount": amt}


def test_bars_to_rows_keeps_latest_session_only():
    """跨两日的 bar → 只留最新一日,且按时间升序。"""
    bars = [
        _bar("2026-06-17 14:45:00", 1, 1, 1, 1, 10, 10.0),   # 旧日 → 应剔除
        _bar("2026-06-18 15:00:00", 2, 2, 2, 2, 20, 20.0),
        _bar("2026-06-18 09:45:00", 3, 3, 3, 3, 30, 30.0),
    ]
    rows = bars_to_rows(bars)
    assert [r["time"] for r in rows] == ["2026-06-18 09:45:00", "2026-06-18 15:00:00"]
    assert rows[0]["open"] == 3 and rows[-1]["close"] == 2


def test_bars_to_rows_empty():
    assert bars_to_rows([]) == []


def test_write_csv_schema_and_none():
    rows = [{"time": "2026-06-18 15:00:00", "open": 1.0, "high": 1.1, "low": 0.9,
             "close": 1.05, "volume": 100, "amount": None}]
    d = tempfile.mkdtemp()
    p = os.path.join(d, "x.csv")
    n = write_csv(p, rows)
    assert n == 1
    lines = open(p, encoding="utf-8").read().splitlines()
    assert lines[0] == "time,open,high,low,close,volume,amount"
    assert lines[1].endswith(",100,")   # amount=None → 空串


def test_tick_to_row_with_quotes():
    """current() tick → 快照行：盘口取 quotes[0],created_at 格式化。"""
    t = {"symbol": "SHSE.600519", "open": 1235.0, "high": 1238.87, "low": 1211.22,
         "price": 1215.0, "cum_volume": 5747173, "cum_amount": 7016713941.0,
         "quotes": [{"bid_p": 1215.0, "bid_v": 12292, "ask_p": 1215.28, "ask_v": 100}],
         "created_at": dt.datetime(2026, 6, 18, 15, 1, 55)}
    r = tick_to_row("sh600519", t)
    assert r["symbol"] == "sh600519"
    assert r["time"] == "2026-06-18 15:01:55"
    assert r["open"] == 1235.0 and r["price"] == 1215.0
    assert r["cum_volume"] == 5747173 and r["cum_amount"] == 7016713941.0
    assert r["bid1"] == 1215.0 and r["bid1_v"] == 12292
    assert r["ask1"] == 1215.28 and r["ask1_v"] == 100


def test_tick_to_row_no_quotes():
    """无盘口 / 无 created_at → 盘口与时间置空,不报错。"""
    r = tick_to_row("sz300750", {"symbol": "SZSE.300750", "price": 400.0,
                                  "quotes": [], "created_at": None})
    assert r["price"] == 400.0
    assert r["bid1"] is None and r["ask1"] is None and r["time"] == ""


def test_write_rows_generic_and_missing_key():
    """通用写出:缺列 → 空串。"""
    d = tempfile.mkdtemp()
    p = os.path.join(d, "snap.csv")
    rows = [{"symbol": "sh600519", "price": 1215.0}]   # 只给两列
    write_rows(p, SNAP_COLS, rows)
    lines = open(p, encoding="utf-8").read().splitlines()
    assert lines[0] == ",".join(SNAP_COLS)
    assert lines[1].split(",")[0] == "sh600519"
    assert lines[1].split(",")[SNAP_COLS.index("price")] == "1215.0"
    assert lines[1].split(",")[SNAP_COLS.index("open")] == ""   # 缺 → 空


if __name__ == "__main__":
    for name, fn in sorted(globals().items()):
        if name.startswith("test_") and callable(fn):
            fn(); print(f"PASS {name}")
    print("all passed")
