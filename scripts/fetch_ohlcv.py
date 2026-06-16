"""逐股拉日线 qfq OHLCV → data/<sym>.csv（引擎 primary 格式，time=收盘 15:00:00）。
默认源 sina (stock_zh_a_daily)：稳、不限频；eastmoney (stock_zh_a_hist) 为退市股备用。
**量纲统一铁律**：volume 一律存"股"。sina 直出股；eastmoney 成交量是"手"(×100)→脚本 ×100 归一。
  否则 sina(股) 与 eastmoney(手) 混入同一横截面会让 close*volume 成交额排名错乱（差 100×）。
resume：已存在且最新(末日在 refresh-within 天内)则跳过(--force 忽略)；否则整段重拉覆盖(qfq 防接缝)。
直连(绕代理) + 连接类错误重试退避 + 无数据(退市)不重试 + 限速 + 失败续跑。
用法: python scripts/fetch_ohlcv.py [--universe ...] [--data-dir ...] [--source sina|em]
      [--start 20180101] [--sleep 0.3] [--retries 3] [--backoff 2.0] [--refresh-within 5] [--force] [--limit N]"""
import argparse, os, sys, time, datetime
# 域内数据源直连更稳：清代理 env + 屏蔽 requests 代理查找（Windows 会回退系统/注册表代理）。
for _k in list(os.environ):
    if "proxy" in _k.lower():
        os.environ.pop(_k, None)
import requests.utils
import requests.sessions
requests.utils.getproxies = lambda: {}
requests.sessions.getproxies = lambda: {}
import akshare as ak
import pandas as pd

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

def _no_data(err):
    """退市/无数据类错误（不该重试）。"""
    s = str(err).lower()
    return "value to decode" in s or "jsondecode" in type(err).__name__.lower()

def fetch_one(sym, start, end, source):
    """→ DataFrame(time,open,high,low,close,volume[股]) 或 None(无数据)。连接类错误向上抛(重试)。"""
    try:
        if source == "em":
            df = ak.stock_zh_a_hist(symbol=sym[2:], period="daily",
                                    start_date=start, end_date=end, adjust="qfq")
            if df is None or df.empty:
                return None
            return pd.DataFrame({
                "time": pd.to_datetime(df["日期"]).dt.strftime("%Y-%m-%d 15:00:00"),
                "open": df["开盘"], "high": df["最高"], "low": df["最低"],
                "close": df["收盘"], "volume": df["成交量"] * 100.0,  # 手→股
            })
        df = ak.stock_zh_a_daily(symbol=sym, start_date=start, end_date=end, adjust="qfq")
        if df is None or df.empty:
            return None
        return pd.DataFrame({
            "time": pd.to_datetime(df["date"]).dt.strftime("%Y-%m-%d 15:00:00"),
            "open": df["open"], "high": df["high"], "low": df["low"],
            "close": df["close"], "volume": df["volume"],  # sina 已是股
        })
    except Exception as e:
        if _no_data(e):
            return None  # 退市/无数据 → 不重试
        raise           # 连接类 → 上层重试

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--universe", default="data/universe_full.csv")
    ap.add_argument("--data-dir", default="data")
    ap.add_argument("--source", default="sina", choices=["sina", "em"])
    ap.add_argument("--start", default="20180101")
    ap.add_argument("--sleep", type=float, default=0.3)
    ap.add_argument("--retries", type=int, default=3)
    ap.add_argument("--backoff", type=float, default=2.0)
    ap.add_argument("--refresh-within", type=int, default=5)
    ap.add_argument("--force", action="store_true")
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
        if not args.force and ld is not None and (today - ld).days <= args.refresh_within:
            skip += 1
            continue
        out = None
        last_err = None
        for attempt in range(args.retries):
            try:
                out = fetch_one(sym, args.start, today_s, args.source)
                last_err = None
                break
            except Exception as e:  # 连接抖动/源丢连接 → 退避重试
                last_err = e
                time.sleep(args.backoff * (attempt + 1))
        if last_err is not None:
            print(f"WARN {sym} failed after {args.retries}: {str(last_err)[:80]}", file=sys.stderr)
            fail += 1
            time.sleep(args.sleep)
            continue
        if out is None or out.empty:  # 退市/窗口外 → 无数据
            skip += 1
            time.sleep(args.sleep)
            continue
        out.to_csv(path, index=False)
        ok += 1
        if (i + 1) % 100 == 0:
            print(f"  {i+1}/{len(syms)} ok={ok} fail={fail} skip={skip}", file=sys.stderr)
        time.sleep(args.sleep)
    print(f"done[{args.source}]: ok={ok} fail={fail} skip={skip} of {len(syms)}")

if __name__ == "__main__":
    main()
