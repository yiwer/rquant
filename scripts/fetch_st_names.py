#!/usr/bin/env python3
"""从 baostock query_all_stock 拉当前全市场名称(code_name 含 ST/*ST 标记)→
重建 data/baostock/st_symbols.csv(当前 ST 名单)+ 合并刷新 data/baostock/stock_names.csv。

ST 状态随"披星戴帽/摘帽"变化,旧名单会滞后(实测 sz002217 已是 ST合力泰 但旧名单漏掉)。
应定期跑(如每周)。baostock code_name 是当前权威源。

跑:python scripts/fetch_st_names.py            # 用今天
    python scripts/fetch_st_names.py 2026-06-23 # 指定交易日
"""
import os, sys, time
import baostock as bs
import pandas as pd

try:
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:
    pass
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ST_PATH = os.path.join(REPO, "data", "baostock", "st_symbols.csv")
NAMES_PATH = os.path.join(REPO, "data", "baostock", "stock_names.csv")


def to_sym(code):            # "sz.002217" -> "sz002217"
    return code.replace(".", "")


def is_stock(sym):           # 排除指数(sh000/sz399)等,仅留可交易A股
    p, n = sym[:2], sym[2:]
    if p == "sh":
        return n.startswith("6") or n.startswith("9")     # 主板/科创(688)
    if p == "sz":
        return n.startswith("0") or n.startswith("30")    # 主板/创业板(300/301);排除 399 指数
    if p == "bj":
        return n.startswith("4") or n.startswith("8") or n.startswith("92")
    return False


def main():
    day = sys.argv[1] if len(sys.argv) > 1 else time.strftime("%Y-%m-%d")
    if bs.login().error_code != "0":
        raise SystemExit("baostock login failed")
    rs = bs.query_all_stock(day=day)
    rows = []
    while rs.error_code == "0" and rs.next():
        rows.append(rs.get_row_data())
    bs.logout()
    if not rows:
        raise SystemExit(f"query_all_stock({day}) 空(非交易日?换一个交易日)")
    df = pd.DataFrame(rows, columns=rs.fields)            # code, tradeStatus, code_name
    df["symbol"] = df["code"].map(to_sym)
    df = df[df["symbol"].map(is_stock)].copy()
    df["name"] = df["code_name"].astype(str)

    # 当前 ST 名单(名称含 ST/*ST)
    st = df[df["name"].str.contains("ST", case=False, na=False)][["symbol", "name"]].sort_values("symbol")
    st.to_csv(ST_PATH, index=False, encoding="utf-8")

    # 名称合并:当前覆盖旧,保留旧文件里已退市/不在当日名单的历史名
    cur = dict(zip(df["symbol"], df["name"]))
    merged = {}
    if os.path.exists(NAMES_PATH):
        old = pd.read_csv(NAMES_PATH, dtype=str)
        merged = dict(zip(old["symbol"], old["name"]))
    merged.update(cur)
    pd.DataFrame(sorted(merged.items()), columns=["symbol", "name"]).to_csv(
        NAMES_PATH, index=False, encoding="utf-8")

    print(f"day={day}  stocks={len(df)}  ST={len(st)} → {ST_PATH}")
    print(f"  names merged={len(merged)} (current {len(cur)} overlaid) → {NAMES_PATH}")
    print(f"  sz002217 当前名: {cur.get('sz002217', '(不在当日名单)')}  | 在ST名单: {'sz002217' in set(st['symbol'])}")


if __name__ == "__main__":
    main()
