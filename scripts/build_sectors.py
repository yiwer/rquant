#!/usr/bin/env python3
"""板块数据：baostock 行业 membership + 用现成日线等权聚合各行业日线序列（收益/指数/广度）。

eastmoney 板块端点限频不通 → 用 baostock query_stock_industry 取行业归属，再从已有 data/<sym>.csv
等权聚合成分股日收益 → 每行业日线序列（survivorship-controllable，比抓板块指数更干净）。
输出：
  data/baostock/sector_membership.csv         symbol,industry,classification,update_date
  data/baostock/sector/<industry>.csv         time,ret,index,n,breadth   （等权日收益/净值/成分数/上涨广度）
  data/baostock/sector_daily_panel.csv        time,industry,ret,n,breadth （长表，横截面板块分析）
行业查询为单次轻量调用（与抓取的重 k 线流并发风险低）；失败则报错，由监控下轮重试。
"""
import argparse, os, sys
import numpy as np
import pandas as pd

try:
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:
    pass
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def fetch_membership(out_path):
    import baostock as bs
    lg = bs.login()
    if lg.error_code != "0":
        raise SystemExit(f"baostock login failed: {lg.error_msg}")
    try:
        rs = bs.query_stock_industry()
        if rs.error_code != "0":
            raise SystemExit(f"query_stock_industry ec={rs.error_code} {rs.error_msg}")
        rows = []
        while rs.error_code == "0" and rs.next():
            rows.append(rs.get_row_data())  # updateDate,code,code_name,industry,industryClassification
    finally:
        bs.logout()
    recs = []
    for r in rows:
        upd, code, name, ind, cls = r[0], r[1], r[2], r[3], r[4]
        if not ind:
            continue  # 未分类跳过
        sym = code.replace(".", "")  # sh.600000 -> sh600000
        recs.append({"symbol": sym, "industry": ind, "classification": cls, "update_date": upd})
    df = pd.DataFrame(recs).sort_values("symbol")
    df.to_csv(out_path, index=False)
    print(f"membership: {len(df)} classified symbols, {df['industry'].nunique()} industries → {out_path}")
    return df


def build_sector_series(mem_df, data_dir, sector_dir, panel_path, from_date):
    os.makedirs(sector_dir, exist_ok=True)
    sym2ind = dict(zip(mem_df["symbol"], mem_df["industry"]))
    parts = []
    n_read = 0
    for sym, ind in sym2ind.items():
        p = os.path.join(data_dir, f"{sym}.csv")
        if not os.path.exists(p):
            continue
        try:
            df = pd.read_csv(p, usecols=["time", "close"])
        except Exception:
            continue
        df["time"] = pd.to_datetime(df["time"])
        df = df[df["time"] >= from_date]
        if len(df) < 2:
            continue
        df = df.sort_values("time")
        ret = df["close"].pct_change()
        parts.append(pd.DataFrame({"date": df["time"].values, "industry": ind, "ret": ret.values}))
        n_read += 1
    print(f"aggregating {n_read} constituents with daily data...")
    long = pd.concat(parts, ignore_index=True).dropna(subset=["ret"])
    # clip 极端（除权/数据异常）防污染均值
    long = long[long["ret"].abs() < 0.5]
    g = long.groupby(["date", "industry"])
    panel = g["ret"].agg(ret="mean", n="count").reset_index()
    breadth = g["ret"].apply(lambda s: float((s > 0).mean())).reset_index(name="breadth")
    panel = panel.merge(breadth, on=["date", "industry"])
    panel = panel.sort_values(["industry", "date"])
    panel.to_csv(panel_path, index=False)
    print(f"sector daily panel: {len(panel)} rows, {panel['industry'].nunique()} industries → {panel_path}")
    # 每行业一个文件（含等权净值 index）
    for ind, sub in panel.groupby("industry"):
        sub = sub.sort_values("date").copy()
        sub["index"] = (1.0 + sub["ret"]).cumprod()
        safe = ind.replace("/", "_").replace("\\", "_")
        out = pd.DataFrame({"time": pd.to_datetime(sub["date"]).dt.strftime("%Y-%m-%d 15:00:00"),
                            "ret": sub["ret"], "index": sub["index"], "n": sub["n"], "breadth": sub["breadth"]})
        out.to_csv(os.path.join(sector_dir, f"{safe}.csv"), index=False)
    print(f"wrote {panel['industry'].nunique()} per-industry sector series → {sector_dir}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--data-dir", default=os.path.join(REPO, "data"))
    ap.add_argument("--out-root", default=os.path.join(REPO, "data", "baostock"))
    ap.add_argument("--from-date", default="2018-01-01")
    ap.add_argument("--membership-only", action="store_true")
    a = ap.parse_args()
    os.makedirs(a.out_root, exist_ok=True)
    mem_path = os.path.join(a.out_root, "sector_membership.csv")
    if os.path.exists(mem_path):
        mem = pd.read_csv(mem_path)
        print(f"membership exists: {len(mem)} rows (reuse)")
    else:
        mem = fetch_membership(mem_path)
    if a.membership_only:
        return
    build_sector_series(mem, a.data_dir, os.path.join(a.out_root, "sector"),
                        os.path.join(a.out_root, "sector_daily_panel.csv"), a.from_date)


if __name__ == "__main__":
    main()
