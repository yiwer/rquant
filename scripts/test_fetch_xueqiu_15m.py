#!/usr/bin/env python3
"""fetch_xueqiu_15m 纯函数单测：代码格式转换 + 时间转换 + CSV 去重往返。

跑：python -m pytest scripts/test_fetch_xueqiu_15m.py -q
或：python scripts/test_fetch_xueqiu_15m.py
不联网（只测纯函数；history() 走网络不在此覆盖）。
"""
import os
import tempfile

from fetch_xueqiu_15m import (to_xq, to_local, date_to_ms, ms_to_str,
                              write_csv, _read_existing_times)


def test_to_xq_market_inference():
    """无前缀按首位判市场：6→SH，0/3→SZ，4/8→BJ。"""
    assert to_xq("600519") == "SH600519"
    assert to_xq("000001") == "SZ000001"
    assert to_xq("300750") == "SZ300750"
    assert to_xq("830799") == "BJ830799"
    assert to_xq("430139") == "BJ430139"


def test_to_xq_prefix_and_suffix_forms():
    """前/后缀、点号、大小写都归一到雪球 SH/SZ 大写。"""
    for s in ("sh600000", "SH600000", "600000.SH", "600000.sh"):
        assert to_xq(s) == "SH600000", s
    for s in ("sz300750", "SZ300750", "300750.SZ"):
        assert to_xq(s) == "SZ300750", s


def test_to_local_is_lowercase_project_form():
    assert to_local("SZ300750") == "sz300750"
    assert to_local("600519.SH") == "sh600519"


def test_time_roundtrip_utc8():
    """date_to_ms→ms_to_str 往返钉死 UTC+8 wall-clock，不依赖本机时区。"""
    assert ms_to_str(date_to_ms("2026-06-18")) == "2026-06-18 00:00:00"
    # 已知一根 15m bar：2026-06-18 15:00 (UTC+8) 对应的毫秒
    ms_1500 = date_to_ms("2026-06-18") + (15 * 3600 + 0 * 60) * 1000
    assert ms_to_str(ms_1500) == "2026-06-18 15:00:00"


def test_write_csv_fresh_then_append_dedup():
    """新建写全量；--update 用 append_times 去重，只追加新 bar。"""
    rows = [
        {"time": "2026-06-18 14:45:00", "open": 1.0, "high": 1.1, "low": 0.9,
         "close": 1.05, "volume": 100, "amount": 105.0},
        {"time": "2026-06-18 15:00:00", "open": 1.05, "high": 1.2, "low": 1.0,
         "close": 1.15, "volume": 200, "amount": 230.0},
    ]
    d = tempfile.mkdtemp()
    path = os.path.join(d, "sz300750.csv")
    n = write_csv(path, rows)
    assert n == 2
    times, last = _read_existing_times(path)
    assert times == {"2026-06-18 14:45:00", "2026-06-18 15:00:00"}
    assert last == "2026-06-18 15:00:00"

    # 再来一根新 + 两根旧：append 模式只应追加新的那一根
    rows2 = rows + [{"time": "2026-06-19 09:45:00", "open": 1.15, "high": 1.3,
                     "low": 1.1, "close": 1.25, "volume": 300, "amount": 375.0}]
    added = write_csv(path, rows2, append_times=times)
    assert added == 1
    times2, last2 = _read_existing_times(path)
    assert len(times2) == 3 and last2 == "2026-06-19 09:45:00"


def test_write_csv_none_becomes_empty():
    """缺失字段 None → 写空串（不臆造 0）。"""
    rows = [{"time": "2026-06-18 15:00:00", "open": 1.0, "high": 1.1, "low": 0.9,
             "close": 1.05, "volume": 100, "amount": None}]
    d = tempfile.mkdtemp()
    path = os.path.join(d, "x.csv")
    write_csv(path, rows)
    with open(path, encoding="utf-8") as f:
        lines = f.read().splitlines()
    assert lines[0] == "time,open,high,low,close,volume,amount"
    assert lines[1].endswith(",100,")  # amount 空


if __name__ == "__main__":
    for name, fn in sorted(globals().items()):
        if name.startswith("test_") and callable(fn):
            fn(); print(f"PASS {name}")
    print("all passed")
