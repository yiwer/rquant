#!/usr/bin/env python3
"""掘金量化(gm SDK) 实时/历史 15m 取数 + 尾盘链路验证 → data/gm/k15m/<sym>.csv（baostock 同构）。

目的：在「不绑券商、只用掘金数据 token」的前提下,验证盘中能否拿到当天 15m,
并实测吞吐,回答「尾盘 14:45–15:00 窗口内全市场拉得完吗」。

四种模式：
  smoke    token 连通 + 拉 1~2 只最近 15m（任意时段可跑；非交易时段返回最近交易日）
  bench    批量逐只拉「最新一节」15m,测 ok率/耗时/吞吐 → 外推到 5115 只 vs 尾盘窗口
  snapshot current() 多只一次拉行情快照 → 写 data/gm/snapshot/snapshot_<时戳>.csv（广度层）
  tail     生产尾盘流：current() 全市场广度快照 +（--shortlist 给的话）漏斗短名单逐只 15m
真正的盘中实时验证须在交易时段跑；非交易时段只验证历史链路与速度。
尾盘架构：广度用 current()(一次多只,快)算快照型因子+漏斗；路径型因子只在短名单上逐只 history。

token：myquant.cn 注册 → 控制台取 token → 存 data/gm/.token（gitignore）/ 环境 GM_TOKEN / --token。
symbol：项目 sh600000/sz300750 → 掘金 SHSE.600000/SZSE.300750。
输出列 time,open,high,low,close,volume,amount 与 data/baostock/k15m 同构。
注意：gm SDK 导入会劫持 stdout,故本脚本先存真 stdout 再导入,所有输出走 _OUT。
"""
import argparse
import datetime as dt
import os
import sys
import time

try:
    sys.stdout.reconfigure(encoding="utf-8")   # Windows GBK 控制台 → utf-8,免中文崩溃
except Exception:
    pass
_OUT = sys.stdout          # gm 导入前先存真 stdout（SDK 会劫持），已是 utf-8
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT_COLS = ["time", "open", "high", "low", "close", "volume", "amount"]


def log(*a):
    print(*a, file=_OUT, flush=True)


def to_gm(sym):
    """sh600000 / SH600000 / 600000.SH / SHSE.600000 → 掘金 SHSE.600000 / SZSE.300750。"""
    s = sym.strip().upper().replace(".", "")
    if s.startswith(("SHSE", "SZSE", "BJSE")):
        return f"{s[:4]}.{s[4:]}"
    if s[:2] in ("SH", "SZ", "BJ"):
        code, mkt = s[2:], s[:2]
    elif s[-2:] in ("SH", "SZ", "BJ"):
        code, mkt = s[:-2], s[-2:]
    else:
        code = s
        mkt = "SH" if code[0] == "6" else ("BJ" if code[0] in ("4", "8") else "SZ")
    exch = {"SH": "SHSE", "SZ": "SZSE", "BJ": "BJSE"}[mkt]
    return f"{exch}.{code}"


def to_local(sym):
    """任意形式 → 项目文件名约定 sh600000（小写）。"""
    g = to_gm(sym)
    exch, code = g.split(".")
    return {"SHSE": "sh", "SZSE": "sz", "BJSE": "bj"}[exch] + code


def bar_time(eob):
    """掘金 bar 的 eob(tz-aware datetime,bar 结束时刻) → 'YYYY-MM-DD HH:MM:SS'。"""
    return eob.strftime("%Y-%m-%d %H:%M:%S")


def write_csv(path, rows):
    return write_rows(path, OUT_COLS, rows)


def write_rows(path, cols, rows):
    if not rows:
        return 0
    with open(path, "w", newline="", encoding="utf-8") as f:
        f.write(",".join(cols) + "\n")
        for r in rows:
            f.write(",".join("" if r.get(c) is None else str(r.get(c)) for c in cols) + "\n")
    return len(rows)


# 快照(current()) 行：广度层 → 算快照型因子(intraday/range_pos/vwap_gap)+ 执行盘口
SNAP_COLS = ["symbol", "time", "open", "high", "low", "price",
             "cum_volume", "cum_amount", "bid1", "bid1_v", "ask1", "ask1_v"]


def tick_to_row(local_sym, t):
    """掘金 current() 的一条 tick(dict-like) → 快照行。无盘口则置空。"""
    t = dict(t)
    qs = t.get("quotes") or []
    q = dict(qs[0]) if qs else {}
    ca = t.get("created_at")
    return {"symbol": local_sym,
            "time": ca.strftime("%Y-%m-%d %H:%M:%S") if ca else "",
            "open": t.get("open"), "high": t.get("high"), "low": t.get("low"),
            "price": t.get("price"), "cum_volume": t.get("cum_volume"),
            "cum_amount": t.get("cum_amount"),
            "bid1": q.get("bid_p"), "bid1_v": q.get("bid_v"),
            "ask1": q.get("ask_p"), "ask1_v": q.get("ask_v")}


def snap_name():
    return "snapshot_" + dt.datetime.now().strftime("%Y%m%d_%H%M") + ".csv"


def load_token(a):
    if a.token:
        return a.token, "arg"
    if a.token_file and os.path.exists(a.token_file) and os.path.getsize(a.token_file) > 0:
        with open(a.token_file, encoding="utf-8") as f:
            return f.read().strip(), f"file:{os.path.relpath(a.token_file, REPO)}"
    if os.environ.get("GM_TOKEN"):
        return os.environ["GM_TOKEN"], "env"
    return None, "none"


def load_symbols(a):
    if a.symbol:
        return list(a.symbol)
    syms = []
    with open(a.symbols, encoding="utf-8") as f:
        for line in f:
            s = line.strip().split(",")[0]
            if s and not s.lower().startswith("symbol"):
                syms.append(s)
    if a.limit and a.limit > 0:
        syms = syms[:a.limit]
    return syms


def bars_to_rows(bars):
    """掘金 history bar 列表 → 输出行(只保留最新一节,即最大日期那天)。"""
    if not bars:
        return []
    rows = []
    for b in bars:
        rows.append({"time": bar_time(b["eob"]), "open": b.get("open"), "high": b.get("high"),
                     "low": b.get("low"), "close": b.get("close"),
                     "volume": b.get("volume"), "amount": b.get("amount")})
    rows.sort(key=lambda r: r["time"])
    last_day = rows[-1]["time"][:10]
    return [r for r in rows if r["time"][:10] == last_day]


def main():
    ap = argparse.ArgumentParser(description="掘金 15m 实时/历史取数 + 尾盘链路验证")
    ap.add_argument("--mode", choices=["smoke", "bench", "snapshot", "tail"], default="smoke")
    ap.add_argument("--symbol", action="append", help="单只(可重复),如 sz300750；优先于 --symbols")
    ap.add_argument("--symbols", default=os.path.join(REPO, "data", "baostock", "universe_5yr_symbols.txt"))
    ap.add_argument("--limit", type=int, default=300, help="bench/snapshot 取清单前 N 只(0=全部)")
    ap.add_argument("--count", type=int, default=20, help="history_n 每只取最近几根 15m")
    ap.add_argument("--chunk", type=int, default=200, help="snapshot 每次 current() 传多少只")
    ap.add_argument("--out-dir", default=os.path.join(REPO, "data", "gm", "k15m"))
    ap.add_argument("--snap-dir", default=os.path.join(REPO, "data", "gm", "snapshot"))
    ap.add_argument("--shortlist", default=None,
                    help="tail 模式:漏斗筛出的短名单文件(每行一只);给了才拉这些只的 15m bars")
    ap.add_argument("--funnel", action="store_true",
                    help="tail 模式:用本次广度快照内部跑漏斗生成短名单(一条命令搞定广度→漏斗→深度)")
    ap.add_argument("--pool", default=None, help="funnel:日线层候选集文件 → 取交集")
    ap.add_argument("--rank", choices=["liquidity", "intraday", "range_pos", "vwap_gap"],
                    default="liquidity", help="funnel 粗排键(默认中性=流动性)")
    ap.add_argument("--top", type=int, default=300, help="funnel 取前 N")
    ap.add_argument("--out", default=os.path.join(REPO, "data", "gm", "shortlist.txt"),
                    help="funnel 短名单输出路径")
    ap.add_argument("--min-price", type=float, default=2.0, help="funnel 门槛:最低价")
    ap.add_argument("--max-price", type=float, default=0.0, help="funnel 门槛:最高价(0=不限)")
    ap.add_argument("--min-amount", type=float, default=3e7, help="funnel 门槛:今日成交额下限(元)")
    ap.add_argument("--drop-limit-up", action="store_true", help="funnel 门槛:剔除涨停封板(无卖盘)")
    ap.add_argument("--token", default=None)
    ap.add_argument("--token-file", default=os.path.join(REPO, "data", "gm", ".token"))
    ap.add_argument("--write", action="store_true", help="bench 时把每只写出 CSV(默认只测不写)")
    a = ap.parse_args()

    token, src = load_token(a)
    if not token:
        log("[!] 无 token。myquant.cn 注册取 token → 存 data/gm/.token / 设 GM_TOKEN / --token")
        sys.exit(2)
    log(f"token source: {src}")

    from gm.api import set_token, history_n, current  # 劫持 stdout,故放函数内、log 用 _OUT
    set_token(token)

    if a.mode == "smoke":
        syms = a.symbol or ["sz300750", "sh600519"]
        log(f"[smoke] {len(syms)} 只 · history_n 900s count={a.count}")
        for s in syms:
            g = to_gm(s)
            t0 = time.time()
            try:
                bars = history_n(symbol=g, frequency="900s", count=a.count, adjust=1, df=False)
            except Exception as e:
                log(f"  调用失败 {to_local(s)} ({g}): {type(e).__name__}: {str(e)[:120]}")
                log("  → 多半是 token 无效/过期/未开通数据权限。去 myquant.cn 控制台核对 token,"
                    "确认账户已激活行情数据,再重试。")
                sys.exit(1)
            rows = bars_to_rows(bars)
            dtms = (time.time() - t0) * 1000
            if rows:
                log(f"  {to_local(s)} ({g}): {len(rows)} 根 [{rows[0]['time']} .. {rows[-1]['time']}] {dtms:.0f}ms")
                log(f"     首根 {rows[0]}")
            else:
                log(f"  {to_local(s)} ({g}): 0 根（休市无当日数据,或该 token 无此权限） {dtms:.0f}ms")
        log("smoke 完成。有数据=链路通；若 0 根且当前是交易时段→多半 token 权限/额度问题。")
        return

    def fetch_snapshot(gm_syms):
        """current() 分批拉 → 快照行列表(只保留有现价的)。"""
        rows = []
        for i in range(0, len(gm_syms), a.chunk):
            ticks = current(symbols=gm_syms[i:i + a.chunk]) or []
            for t in ticks:
                td = dict(t)
                if td.get("price"):
                    rows.append(tick_to_row(to_local(td["symbol"]), td))
        return rows

    if a.mode == "snapshot":
        gm_syms = [to_gm(s) for s in load_symbols(a)]
        os.makedirs(a.snap_dir, exist_ok=True)
        log(f"[snapshot] current() {len(gm_syms)} 只 · 每批 {a.chunk}")
        t0 = time.time()
        rows = fetch_snapshot(gm_syms)
        el = time.time() - t0
        out = os.path.join(a.snap_dir, snap_name())
        write_rows(out, SNAP_COLS, rows)
        log(f"  {len(rows)}/{len(gm_syms)} 只有效 · {el:.1f}s · {len(rows)/max(el,1e-9):.0f} 只/秒 "
            f"→ {os.path.relpath(out, REPO)}")
        log(f"  → 全市场 5115 只约 {el/max(len(gm_syms),1)*5115:.1f}s（1 请求多只,广度路径）")
        return

    if a.mode == "tail":
        # 广度：current() 全 universe → 快照 CSV（算快照型因子 + 漏斗）
        gm_syms = [to_gm(s) for s in load_symbols(a)]
        os.makedirs(a.snap_dir, exist_ok=True)
        log(f"[tail] 广度 current() {len(gm_syms)} 只 …")
        t0 = time.time()
        rows = fetch_snapshot(gm_syms)
        out = os.path.join(a.snap_dir, snap_name())
        write_rows(out, SNAP_COLS, rows)
        log(f"  广度: {len(rows)}/{len(gm_syms)} 只 · {time.time()-t0:.1f}s → {os.path.relpath(out, REPO)}")

        # 决定短名单：--funnel 用本次快照内部漏斗；否则用 --shortlist 文件
        short = None
        if a.funnel:
            # 延迟导入避免与 build_gm_shortlist 的 `from fetch_gm_realtime import` 循环依赖
            from build_gm_shortlist import passes_gates, select_top, load_pool
            gated = [r for r in rows if passes_gates(r, a.min_price, a.max_price,
                                                      a.min_amount, a.drop_limit_up)]
            n1 = len(gated)
            poolmsg = ""
            if a.pool and os.path.exists(a.pool):
                pool = load_pool(a.pool)
                gated = [r for r in gated if r["symbol"] in pool]
                poolmsg = f" pool∩{len(gated)}"
            picked = select_top(gated, a.rank, a.top)
            short = [r["symbol"] for r in picked]
            with open(a.out, "w", encoding="utf-8") as f:
                f.write("".join(s + "\n" for s in short))
            log(f"  漏斗: 门槛后{n1}{poolmsg} → 粗排[{a.rank}]取前{a.top} = {len(short)} → {os.path.relpath(a.out, REPO)}")
        elif a.shortlist and os.path.exists(a.shortlist):
            short = []
            with open(a.shortlist, encoding="utf-8") as f:
                for line in f:
                    s = line.strip().split(",")[0]
                    if s and not s.lower().startswith("symbol"):
                        short.append(s)

        # 深度：短名单 → history_n 15m（路径型因子）
        if short:
            os.makedirs(a.out_dir, exist_ok=True)
            log(f"  深度: 短名单 {len(short)} 只 · history_n 15m count={a.count} …")
            t1 = time.time(); ok = 0
            for s in short:
                try:
                    bars = history_n(symbol=to_gm(s), frequency="900s", count=a.count, adjust=1, df=False)
                    r = bars_to_rows(bars)
                    if r:
                        write_csv(os.path.join(a.out_dir, f"{to_local(s)}.csv"), r); ok += 1
                except Exception as e:
                    if ok == 0:
                        log(f"    err {s}: {type(e).__name__}: {str(e)[:60]}")
            log(f"  深度: {ok}/{len(short)} 只写出 15m · {time.time()-t1:.1f}s → {os.path.relpath(a.out_dir, REPO)}")
        else:
            log("  (无 --funnel 也无 --shortlist → 只出广度快照)")
        return

    # bench
    syms = load_symbols(a)
    os.makedirs(a.out_dir, exist_ok=True)
    log(f"[bench] {len(syms)} 只 · history_n 900s count={a.count} · write={a.write}")
    t0 = time.time(); ok = fail = 0; nbars = 0
    for i, s in enumerate(syms, 1):
        try:
            bars = history_n(symbol=to_gm(s), frequency="900s", count=a.count, adjust=1, df=False)
            rows = bars_to_rows(bars)
            if rows:
                ok += 1; nbars += len(rows)
                if a.write:
                    write_csv(os.path.join(a.out_dir, f"{to_local(s)}.csv"), rows)
            else:
                fail += 1
        except Exception as e:
            fail += 1
            if fail <= 5:
                log(f"  err {s}: {type(e).__name__}: {str(e)[:80]}")
        if ok == 0 and fail >= 10:   # 前 10 只全失败 → 疑似 token 无效/无权限,早停不空跑 5115 次
            log("  连续失败 ≥10 且 0 成功 → 疑似 token 无效/无权限/限频,中止。先用 --mode smoke 确认链路。")
            sys.exit(1)
        if i % 50 == 0:
            el = time.time() - t0
            log(f"  [{i}/{len(syms)}] ok={ok} fail={fail} · {i/max(el,1e-9):.2f} 只/秒")
    el = time.time() - t0
    rate = len(syms) / max(el, 1e-9)
    full = 5115 / max(rate, 1e-9)
    log(f"\n[bench 结果] {len(syms)} 只 in {el:.1f}s · ok={ok} fail={fail} · "
        f"{nbars/max(ok,1):.1f} 根/只 · {rate:.2f} 只/秒")
    log(f"  → 外推全市场 5115 只 ≈ {full/60:.1f} 分钟（单线程 REST）")
    win = 15 * 60
    verdict = "可行" if full <= win else f"不可行,需 ≥{full/win:.1f}x 并发 或 改用快照/订阅(push)"
    log(f"  → 尾盘窗口 14:45–15:00 = 15min → {verdict}")


if __name__ == "__main__":
    main()
