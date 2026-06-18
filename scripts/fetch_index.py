#!/usr/bin/env python3
"""抓主流宽基指数日线 → data/baostock/index/<code>.csv（time,close）。

用于迭代 harness 的"换框架重测"：把选股组合的超额从『等权小盘 beta 强基准』
改测对『可交易宽基指数』(CSI300/500/1000) 的真 alpha。指数不复权(adjustflag=3)。
small：3 序列 × ~2000 日，秒级。可与逐股抓取并发（量极小）。
"""
import os, socket, sys, time
socket.setdefaulttimeout(60)
import baostock as bs
import pandas as pd

try:
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:
    pass
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(REPO, "data", "baostock", "index")

# code → 友好名（仅日志）。sh.000300 沪深300(大盘) / sh.000905 中证500(中盘) / sh.000852 中证1000(小盘)
INDICES = {"sh.000300": "csi300", "sh.000905": "csi500", "sh.000852": "csi1000"}
FIELDS = "date,close"
FROM, TO = "2018-01-01", time.strftime("%Y-%m-%d")


def main():
    os.makedirs(OUT, exist_ok=True)
    lg = bs.login()
    if lg.error_code != "0":
        raise SystemExit(f"baostock login failed: {lg.error_msg}")
    for code, name in INDICES.items():
        rs = bs.query_history_k_data_plus(code, FIELDS, start_date=FROM, end_date=TO,
                                          frequency="d", adjustflag="3")  # 3=不复权(指数)
        if rs.error_code != "0":
            print(f"  {name}({code}) ERROR ec={rs.error_code} {rs.error_msg}", flush=True)
            continue
        rows = []
        while rs.error_code == "0" and rs.next():
            rows.append(rs.get_row_data())
        df = pd.DataFrame(rows, columns=["date", "close"])
        df["close"] = pd.to_numeric(df["close"], errors="coerce")
        df = df.dropna()
        df["time"] = df["date"] + " 15:00:00"
        df[["time", "close"]].to_csv(os.path.join(OUT, f"{name}.csv"), index=False)
        print(f"  {name}({code}) {len(df)} rows {df['date'].iloc[0]}..{df['date'].iloc[-1]}", flush=True)
    bs.logout()
    print("DONE index fetch")


if __name__ == "__main__":
    main()
