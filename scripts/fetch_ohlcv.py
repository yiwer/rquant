"""逐股拉日线 qfq OHLCV → data/<sym>.csv（引擎 primary 格式，time=收盘 15:00:00）。
resume: 已存在且最新(末日在 refresh-within 天内)则跳过；否则整段重拉覆盖(qfq 防接缝)。
直连(绕代理) + 每股重试退避 + 限速 + 失败续跑（eastmoney 高频丢连接 / 本地 VPN 隧道不稳）。
用法: python scripts/fetch_ohlcv.py [--universe data/universe_full.csv] [--data-dir data]
      [--start 20180101] [--sleep 0.6] [--retries 4] [--backoff 2.0] [--refresh-within 5] [--limit N(0=all)]"""
import argparse, os, sys, time, datetime
# 域内 eastmoney 直连更稳：清除代理 env + 屏蔽 requests 的代理查找
# （Windows 上 requests 会在 env 缺失时回退系统/注册表代理，故须 patch getproxies）。
for _k in list(os.environ):
    if "proxy" in _k.lower():
        os.environ.pop(_k, None)
import requests.utils
import requests.sessions
requests.utils.getproxies = lambda: {}
requests.sessions.getproxies = lambda: {}
import akshare as ak
import pandas as pd

def code6(sym):
    return sym[2:] if sym[:2] in ("sh", "sz") else sym

def last_date(path):
    if not os.path.exists(path):
        return None
    try:
        df = pd.read_csv(path)
        if df.empty:
            return None
        return pd.to_datetime(df["time"].iloc[-1]).date()
    except Exception:
        return None

def fetch_one(sym, start, end):
    df = ak.stock_zh_a_hist(symbol=code6(sym), period="daily",
                            start_date=start, end_date=end, adjust="qfq")
    if df is None or df.empty:
        return None
    return pd.DataFrame({
        "time": pd.to_datetime(df["日期"]).dt.strftime("%Y-%m-%d 15:00:00"),
        "open": df["开盘"], "high": df["最高"], "low": df["最低"],
        "close": df["收盘"], "volume": df["成交量"],
    })

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--universe", default="data/universe_full.csv")
    ap.add_argument("--data-dir", default="data")
    ap.add_argument("--start", default="20180101")
    ap.add_argument("--sleep", type=float, default=0.6)
    ap.add_argument("--retries", type=int, default=4)
    ap.add_argument("--backoff", type=float, default=2.0)
    ap.add_argument("--refresh-within", type=int, default=5)
    ap.add_argument("--limit", type=int, default=0)
    args = ap.parse_args()
    os.makedirs(args.data_dir, exist_ok=True)
    syms = list(pd.read_csv(args.universe)["symbol"])
    if args.limit:
        syms = syms[:args.limit]
    today = datetime.date.today()
    today_s = today.strftime("%Y%m%d")
    ok = fail = skip = 0
    for i, sym in enumerate(syms):
        path = os.path.join(args.data_dir, f"{sym}.csv")
        ld = last_date(path)
        if ld is not None and (today - ld).days <= args.refresh_within:
            skip += 1
            continue
        out = None
        last_err = None
        for attempt in range(args.retries):
            try:
                out = fetch_one(sym, args.start, today_s)
                last_err = None
                break
            except Exception as e:  # 网络抖动/eastmoney 丢连接 → 退避重试
                last_err = e
                time.sleep(args.backoff * (attempt + 1))
        if last_err is not None:
            print(f"WARN {sym} failed after {args.retries}: {str(last_err)[:80]}", file=sys.stderr)
            fail += 1
            time.sleep(args.sleep)
            continue
        if out is None or out.empty:  # 退市/窗口外 → 无数据，跳过
            skip += 1
            time.sleep(args.sleep)
            continue
        out.to_csv(path, index=False)
        ok += 1
        if (i + 1) % 100 == 0:
            print(f"  {i+1}/{len(syms)} ok={ok} fail={fail} skip={skip}", file=sys.stderr)
        time.sleep(args.sleep)
    print(f"done: ok={ok} fail={fail} skip={skip} of {len(syms)}")

if __name__ == "__main__":
    main()
