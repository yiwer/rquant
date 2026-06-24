#!/usr/bin/env python3
"""增量更新已有 kday 日线到最新(对每个已存在文件追加尾部缺失的交易日)。

fetch_baostock.py 是"文件存在即跳过"的全量/续传器,不追加新日期。本脚本对
data/baostock/kday/ 下每个已有 CSV:读最后日期 → baostock 取 (末日+1 .. --to)
qfq 日线 → 去重排序追加写回。仅日线(因子面板只用日线)。收盘后/数据稳定时跑。

跑:python scripts/fetch_baostock_update.py            # 到今天
    python scripts/fetch_baostock_update.py --to 2026-06-24
"""
import argparse, glob, os, socket, sys, time
socket.setdefaulttimeout(60)
import baostock as bs
import pandas as pd

try:
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:
    pass
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DAY_FIELDS = "date,open,high,low,close,volume,amount,turn,pctChg"
COLS = ["time", "open", "high", "low", "close", "volume", "amount", "turn", "pctChg"]


def to_bs(sym):
    return sym[:2] + "." + sym[2:]


def _login():
    return bs.login().error_code == "0"


def _last_date(path):
    """读 CSV 最后一行的日期(YYYY-MM-DD)。空/坏文件返 None。"""
    try:
        df = pd.read_csv(path, usecols=["time"])
        if df.empty:
            return None
        return str(df["time"].iloc[-1])[:10]
    except Exception:
        return None


def _next_day(d):
    y, m, da = map(int, d.split("-"))
    import datetime as dt
    return (dt.date(y, m, da) + dt.timedelta(days=1)).strftime("%Y-%m-%d")


def update_one(sym, day_dir, end, retries=3):
    dp = os.path.join(day_dir, f"{sym}.csv")
    last = _last_date(dp)
    if last is None:
        return "nofile"
    if last >= end:
        return "current"
    start = _next_day(last)
    sb = to_bs(sym)
    for attempt in range(retries):
        try:
            rs = bs.query_history_k_data_plus(sb, DAY_FIELDS, start_date=start, end_date=end,
                                              frequency="d", adjustflag="2")
            if rs.error_code != "0":
                raise RuntimeError(f"ec={rs.error_code}")
            rows = []
            while rs.error_code == "0" and rs.next():
                rows.append(rs.get_row_data())
            if not rows:
                return "current"
            out = [[f"{r[0]} 15:00:00"] + r[1:] for r in rows]
            new = pd.DataFrame(out, columns=COLS)
            for c in COLS[1:]:
                new[c] = pd.to_numeric(new[c], errors="coerce")
            new = new.dropna(subset=["open", "high", "low", "close", "volume"])
            if new.empty:
                return "current"
            old = pd.read_csv(dp)
            merged = (pd.concat([old, new], ignore_index=True)
                      .drop_duplicates("time", keep="last")
                      .sort_values("time"))
            merged.to_csv(dp, index=False)
            return f"+{len(new)}"
        except Exception as e:
            time.sleep(2.0 * (attempt + 1))
            if not _login():
                time.sleep(3.0); _login()
            last_err = str(e)[:80]
    return f"FAIL:{last_err}"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--day-dir", default=os.path.join(REPO, "data", "baostock", "kday"))
    ap.add_argument("--to", default=time.strftime("%Y-%m-%d"))
    ap.add_argument("--limit", type=int, default=0)
    a = ap.parse_args()
    files = sorted(glob.glob(os.path.join(a.day_dir, "*.csv")))
    syms = [os.path.splitext(os.path.basename(f))[0] for f in files]
    if a.limit > 0:
        syms = syms[:a.limit]
    if not _login():
        raise SystemExit("baostock login failed")
    print(f"update {len(syms)} kday files → {a.to}", flush=True)
    upd = cur = fail = 0; added = 0; failed = []
    for i, s in enumerate(syms, 1):
        st = update_one(s, a.day_dir, a.to)
        if st.startswith("+"):
            upd += 1; added += int(st[1:])
        elif st == "current":
            cur += 1
        elif st.startswith("FAIL"):
            fail += 1; failed.append((s, st))
        if i % 50 == 0 or i == len(syms):
            print(f"  [{i}/{len(syms)}] updated={upd} current={cur} fail={fail} bars+={added}", flush=True)
    bs.logout()
    if failed:
        print("failed(first 20):", ", ".join(f"{s}({m})" for s, m in failed[:20]))
    print(f"DONE updated={upd} current={cur} fail={fail} bars_added={added}")


if __name__ == "__main__":
    main()
