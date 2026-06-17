#!/usr/bin/env python3
"""逐股 baostock 抓 qfq 日线(2018+) + 15m(2021+) → data/baostock/{kday,k15m}/<sym>.csv。

baostock 多年日内（实测 5yr 15m ~26s/股，顺序；并发不可行）。survivorship-free（含退市）。
列：日线 time,open,high,low,close,volume,amount,turn,pctChg ；15m time,open,high,low,close,volume,amount。
time 统一 "YYYY-MM-DD HH:MM:SS"（日线 15:00:00）。resume 跳过已存在非空；每股退避重试；login 复用+失效重连。
仅在收盘后/数据稳定时联网跑。
"""
import argparse, os, sys, time
import baostock as bs
import pandas as pd

try:
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:
    pass
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

DAY_FIELDS = "date,open,high,low,close,volume,amount,turn,pctChg"
MIN_FIELDS = "date,time,open,high,low,close,volume,amount"


def to_bs(sym):  # shXXXXXX -> sh.XXXXXX
    return sym[:2] + "." + sym[2:]


def _login():
    lg = bs.login()
    return lg.error_code == "0"


def _query(sym_bs, fields, start, end, freq):
    rs = bs.query_history_k_data_plus(sym_bs, fields, start_date=start, end_date=end,
                                      frequency=freq, adjustflag="2")  # 2=qfq
    if rs.error_code != "0":
        return None, rs.error_code
    rows = []
    while rs.error_code == "0" and rs.next():
        rows.append(rs.get_row_data())
    return rows, rs.error_code


def _fmt_time(date, t=None):
    if t:  # 15m: t like 20210104094500000
        return f"{date} {t[8:10]}:{t[10:12]}:{t[12:14]}"
    return f"{date} 15:00:00"


def _write(path, rows, cols, has_time):
    out = []
    for r in rows:
        if has_time:
            tm = _fmt_time(r[0], r[1]); rest = r[2:]
        else:
            tm = _fmt_time(r[0]); rest = r[1:]
        out.append([tm] + rest)
    df = pd.DataFrame(out, columns=cols)
    for c in cols[1:]:
        df[c] = pd.to_numeric(df[c], errors="coerce")
    df = df.dropna(subset=["open", "high", "low", "close"])
    df.to_csv(path, index=False)
    return len(df)


def fetch_one(sym, day_dir, min_dir, day_from, min_from, end, retries=3):
    sb = to_bs(sym)
    dp = os.path.join(day_dir, f"{sym}.csv"); mp = os.path.join(min_dir, f"{sym}.csv")
    need_d = not (os.path.exists(dp) and os.path.getsize(dp) > 80)
    need_m = not (os.path.exists(mp) and os.path.getsize(mp) > 80)
    if not need_d and not need_m:
        return "skip"
    status = []
    for attempt in range(retries):
        try:
            if need_d:
                rows, ec = _query(sb, DAY_FIELDS, day_from, end, "d")
                if ec != "0":
                    raise RuntimeError(f"day ec={ec}")
                if rows:
                    n = _write(dp, rows, ["time", "open", "high", "low", "close", "volume", "amount", "turn", "pctChg"], False)
                    status.append(f"d={n}")
                else:
                    status.append("d=0")
                need_d = False
            if need_m:
                rows, ec = _query(sb, MIN_FIELDS, min_from, end, "15")
                if ec != "0":
                    raise RuntimeError(f"15m ec={ec}")
                if rows:
                    n = _write(mp, rows, ["time", "open", "high", "low", "close", "volume", "amount"], True)
                    status.append(f"15m={n}")
                else:
                    status.append("15m=0")
                need_m = False
            return ",".join(status) or "ok"
        except Exception as e:
            time.sleep(2.0 * (attempt + 1))
            if not _login():  # 会话可能失效，重连
                time.sleep(3.0); _login()
            last = str(e)[:80]
    return f"FAIL:{last}"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--symbols", default=os.path.join(REPO, "data", "baostock", "universe_5yr_symbols.txt"))
    ap.add_argument("--day-dir", default=os.path.join(REPO, "data", "baostock", "kday"))
    ap.add_argument("--min-dir", default=os.path.join(REPO, "data", "baostock", "k15m"))
    ap.add_argument("--day-from", default="2018-01-01")
    ap.add_argument("--min-from", default="2021-01-01")
    ap.add_argument("--to", default=time.strftime("%Y-%m-%d"))
    ap.add_argument("--limit", type=int, default=0, help=">0 仅前 N（smoke 用）")
    a = ap.parse_args()
    os.makedirs(a.day_dir, exist_ok=True); os.makedirs(a.min_dir, exist_ok=True)
    syms = [s.strip() for s in open(a.symbols, encoding="utf-8") if s.strip()]
    if a.limit > 0:
        syms = syms[:a.limit]
    if not _login():
        raise SystemExit("baostock login failed")
    print(f"baostock fetch {len(syms)} symbols  day≥{a.day_from} 15m≥{a.min_from} → {a.to}", flush=True)
    ok = skip = fail = 0; failed = []
    for i, s in enumerate(syms, 1):
        st = fetch_one(s, a.day_dir, a.min_dir, a.day_from, a.min_from, a.to)
        if st == "skip":
            skip += 1
        elif st.startswith("FAIL"):
            fail += 1; failed.append((s, st))
        else:
            ok += 1
        if i % 10 == 0 or i == len(syms):
            print(f"  [{i}/{len(syms)}] ok={ok} skip={skip} fail={fail}", flush=True)
    bs.logout()
    if failed:
        print("failed(first 20):", ", ".join(f"{s}({m})" for s, m in failed[:20]))
    print(f"DONE ok={ok} skip={skip} fail={fail}")


if __name__ == "__main__":
    main()
