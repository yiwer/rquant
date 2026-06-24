#!/usr/bin/env python3
"""逐股拉雪球 15m K 线 → data/xueqiu/k15m/<sym>.csv（time,open,high,low,close,volume,amount）。

源：stock.xueqiu.com/v5/stock/chart/kline.json（period=15m, type=before 前复权）。
列与 data/baostock/k15m 同构，可被 build_intraday_*.py 直接消费（重定向 K15M 即可）。
bar 标签 09:45..11:30,13:15..15:00（16 根/日，区间末标注），与 baostock 完全对齐。

鉴权：雪球接口要 xq_a_token cookie，否则 error_code=400016「未登录」。
  自动获取：先 GET https://xueqiu.com/hq（下发 guest token 全套），再调接口；
  中途失效(400016)自动重取。也可 --cookie / 环境变量 XQ_COOKIE 传浏览器登录态
  （历史更长、限频更松；从 DevTools 该请求的 Cookie 头整段复制即可）。

历史：begin=now,count=-N 取最近 N 根；用本页最早 ts 作新 begin 往前翻，
  到 --start 日期或无新数据为止。相邻页有 1 根重叠 bar，按 timestamp 去重。

resume：默认跳过已存在非空文件；--force 重抓；--update 只把最新缺的 bar 追加去重。
诚实：无数据/失败→记录跳过，不臆造。Windows 回退系统代理→patch getproxies。
仅在收盘后/数据稳定时联网跑。
"""
import argparse
import datetime as dt
import os
import sys
import time

import requests

requests.utils.getproxies = lambda: {}
try:
    requests.sessions.getproxies = lambda: {}
except Exception:
    pass
try:
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:
    pass

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
KLINE_URL = "https://stock.xueqiu.com/v5/stock/chart/kline.json"
TOKEN_URL = "https://xueqiu.com/hq"
INDICATOR = "kline,pe,pb,ps,pcf,market_capital,agt,ggt,balance"
UA = ("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
      "(KHTML, like Gecko) Chrome/124.0 Safari/537.36")
OUT_COLS = ["time", "open", "high", "low", "close", "volume", "amount"]
CN_TZ = dt.timezone(dt.timedelta(hours=8))  # 固定 UTC+8 wall-clock，避免依赖本机时区


def to_xq(sym):
    """sh600000 / SH600000 / 600000.SH / sz300750 → 雪球 SH600000 / SZ300750。"""
    s = sym.strip().upper().replace(".", "")
    if s[:2] in ("SH", "SZ", "BJ"):
        code, mkt = s[2:], s[:2]
    elif s[-2:] in ("SH", "SZ", "BJ"):
        code, mkt = s[:-2], s[-2:]
    else:
        code = s
        mkt = "SH" if code[0] == "6" else ("BJ" if code[0] in ("4", "8") else "SZ")
    return f"{mkt}{code}"


def to_local(sym):
    """统一成项目文件名约定（小写 sh600000）。"""
    return to_xq(sym).lower()


def ms_to_str(ms):
    return dt.datetime.fromtimestamp(ms / 1000, CN_TZ).strftime("%Y-%m-%d %H:%M:%S")


def date_to_ms(d):
    """'YYYY-MM-DD' → 当日 00:00 (UTC+8) 的毫秒 epoch。"""
    t = dt.datetime.strptime(d, "%Y-%m-%d").replace(tzinfo=CN_TZ)
    return int(t.timestamp() * 1000)


class Xueqiu:
    def __init__(self, cookie=None, page_sleep=0.6, timeout=15):
        self.s = requests.Session()
        self.s.headers.update({
            "User-Agent": UA,
            "Accept": "application/json, text/plain, */*",
            "Accept-Language": "zh-CN,zh;q=0.9,en;q=0.8",
            "Referer": "https://xueqiu.com/",
        })
        self.page_sleep = page_sleep
        self.timeout = timeout
        self._manual_cookie = cookie
        self._ensure_token(initial=True)

    def _ensure_token(self, initial=False):
        if self._manual_cookie:
            # 整段 Cookie 头：交给 session 持有
            for kv in self._manual_cookie.split(";"):
                if "=" in kv:
                    k, v = kv.strip().split("=", 1)
                    self.s.cookies.set(k, v, domain=".xueqiu.com")
            if "xq_a_token" in self.s.cookies.get_dict():
                return
            # 手填 cookie 里没有 token → 退回自动获取
        for attempt in range(3):
            try:
                self.s.get(TOKEN_URL, timeout=self.timeout)
            except Exception:
                pass
            if "xq_a_token" in self.s.cookies.get_dict():
                return
            time.sleep(1.5 * (attempt + 1))
        if initial:
            raise RuntimeError("无法获取 xq_a_token（雪球可能临时风控）；可用 --cookie 传浏览器登录态")

    def _get(self, begin, count, symbol, retries=4):
        last = None
        for k in range(retries):
            try:
                p = {"symbol": symbol, "begin": begin, "period": "15m", "type": "before",
                     "count": count, "indicator": INDICATOR}
                r = self.s.get(KLINE_URL, params=p, timeout=self.timeout)
                if r.status_code in (403, 429):
                    last = f"HTTP {r.status_code}"
                    time.sleep(3.0 * (k + 1)); self._ensure_token(); continue
                j = r.json()
                ec = j.get("error_code")
                if ec in (0, "0", None):
                    return j.get("data") or {}
                last = f"error_code={ec} {j.get('error_description')}"
                if str(ec) == "400016":  # token 失效/风控 → 重取后退避
                    self._ensure_token(); time.sleep(2.0 * (k + 1)); continue
                time.sleep(1.5 * (k + 1))
            except Exception as e:
                last = f"{type(e).__name__}: {str(e)[:80]}"
                time.sleep(2.0 * (k + 1))
        raise RuntimeError(last or "fail")

    def history(self, sym, start_ms, max_pages=400, count=284, stop_at_ms=None):
        """向前翻页收齐 [start_ms, now]（或 > stop_at_ms）的 bar；返回 ts 升序去重行。"""
        xq = to_xq(sym)
        begin = int(time.time() * 1000)
        seen = {}
        for _ in range(max_pages):
            data = self._get(begin, -count, xq)
            cols = data.get("column"); items = data.get("item") or []
            if not items:
                break
            idx = {c: i for i, c in enumerate(cols)}
            new_earliest = None
            added = 0
            for it in items:
                ts = it[idx["timestamp"]]
                if ts not in seen:
                    seen[ts] = it
                    added += 1
                if new_earliest is None or ts < new_earliest:
                    new_earliest = ts
            earliest = min(seen)
            if earliest <= start_ms:
                break
            if stop_at_ms is not None and earliest <= stop_at_ms:
                break
            if added == 0 or new_earliest >= begin:  # 无进展 → 防死循环
                break
            begin = new_earliest  # 下一页右边界 = 本页最早 ts
            time.sleep(self.page_sleep)
        rows = []
        idx = {c: i for i, c in enumerate(cols)} if cols else {}
        for ts in sorted(seen):
            if ts < start_ms or not idx:
                continue
            it = seen[ts]
            rows.append({
                "time": ms_to_str(ts),
                "open": it[idx["open"]], "high": it[idx["high"]],
                "low": it[idx["low"]], "close": it[idx["close"]],
                "volume": it[idx["volume"]], "amount": it[idx["amount"]],
            })
        return rows


def _read_existing_times(path):
    times = set()
    last = None
    try:
        with open(path, encoding="utf-8") as f:
            next(f, None)  # header
            for line in f:
                t = line.split(",", 1)[0]
                if t:
                    times.add(t); last = t
    except Exception:
        pass
    return times, last


def write_csv(path, rows, append_times=None):
    new = [r for r in rows if not append_times or r["time"] not in append_times]
    if not new:
        return 0
    exists = os.path.exists(path) and append_times is not None
    mode = "a" if exists else "w"
    with open(path, mode, newline="", encoding="utf-8") as f:
        if not exists:
            f.write(",".join(OUT_COLS) + "\n")
        for r in new:
            f.write(",".join("" if r[c] is None else str(r[c]) for c in OUT_COLS) + "\n")
    return len(new)


def load_symbols(args):
    if args.symbol:
        return [s for s in args.symbol]
    out = []
    with open(args.symbols, encoding="utf-8") as f:
        for line in f:
            s = line.strip().split(",")[0]
            if s and not s.lower().startswith("symbol"):
                out.append(s)
    return out


def main():
    ap = argparse.ArgumentParser(description="雪球 15m K 线抓取 → CSV")
    ap.add_argument("--symbol", action="append", help="单只代码（可重复），如 sz300750；优先于 --symbols")
    ap.add_argument("--symbols", default=os.path.join(REPO, "data", "baostock", "universe_5yr_symbols.txt"),
                    help="代码清单文件（每行一只，sh600000/sz300750；忽略 header 行）")
    ap.add_argument("--out-dir", default=os.path.join(REPO, "data", "xueqiu", "k15m"))
    ap.add_argument("--start", default="2021-01-01", help="历史起始日 YYYY-MM-DD")
    ap.add_argument("--cookie", default=os.environ.get("XQ_COOKIE"),
                    help="浏览器 Cookie 头整段（含 xq_a_token）；不填则自动取 guest token")
    ap.add_argument("--cookie-file", default=os.path.join(REPO, "data", "xueqiu", ".cookie"),
                    help="cookie 文件路径（整段 Cookie 头，已 gitignore）；存在且非空则优先于 --cookie/env")
    ap.add_argument("--update", action="store_true", help="已有文件只追加最新缺的 bar（增量）")
    ap.add_argument("--force", action="store_true", help="无视已有文件强制重抓")
    ap.add_argument("--sleep", type=float, default=1.2, help="股间隔秒")
    ap.add_argument("--page-sleep", type=float, default=0.6, help="翻页间隔秒")
    ap.add_argument("--max-pages", type=int, default=400, help="单股最大翻页数（安全上限）")
    a = ap.parse_args()

    os.makedirs(a.out_dir, exist_ok=True)
    syms = load_symbols(a)
    start_ms = date_to_ms(a.start)
    cookie, src = a.cookie, "env/arg" if a.cookie else "guest(auto /hq)"
    if a.cookie_file and os.path.exists(a.cookie_file) and os.path.getsize(a.cookie_file) > 0:
        with open(a.cookie_file, encoding="utf-8") as f:
            cookie = f.read().strip()
        src = f"file:{os.path.relpath(a.cookie_file, REPO)}"
    print(f"雪球 15m：{len(syms)} 只 → {a.out_dir}（start={a.start}, update={a.update}, force={a.force}, cookie={src}）")

    xq = Xueqiu(cookie=cookie, page_sleep=a.page_sleep)
    print("token ok:", "xq_a_token" in xq.s.cookies.get_dict(),
          "| logged-in:", bool(cookie and "u=" in (cookie or "")))

    ok = skip = fail = 0
    failed = []
    for i, sym in enumerate(syms, 1):
        out = os.path.join(a.out_dir, f"{to_local(sym)}.csv")
        append_times = None
        stop_at = None
        if os.path.exists(out) and os.path.getsize(out) > 50:
            if a.update:
                append_times, last = _read_existing_times(out)
                # 已有最后一根的前一日作为下界，少翻几页
                if last:
                    stop_at = date_to_ms(last[:10]) - 86400_000
            elif not a.force:
                skip += 1
                continue
        try:
            rows = xq.history(sym, start_ms, max_pages=a.max_pages, stop_at_ms=stop_at)
            if not rows:
                fail += 1; failed.append((sym, "empty"));
            else:
                n = write_csv(out, rows, append_times=append_times)
                ok += 1
                if i <= 3 or i % 50 == 0:
                    span = f"{rows[0]['time'][:10]}..{rows[-1]['time'][:10]}"
                    print(f"  {to_local(sym)}: +{n} bars ({span}, total {len(rows)})", flush=True)
        except Exception as e:
            fail += 1; failed.append((sym, f"{type(e).__name__}: {str(e)[:60]}"))
        if i % 20 == 0 or i == len(syms):
            print(f"  [{i}/{len(syms)}] ok={ok} skip={skip} fail={fail}", flush=True)
        time.sleep(a.sleep)

    if failed:
        print("failed:", ", ".join(f"{s}({m})" for s, m in failed[:20]))
    print(f"DONE ok={ok} skip={skip} fail={fail}")


if __name__ == "__main__":
    main()
