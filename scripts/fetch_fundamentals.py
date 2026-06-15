"""拉 A 股全市场季度基本面 (akshare stock_yjbb_em) → 逐股 point-in-time CSV (公告日锚)。
用法: python scripts/fetch_fundamentals.py [--out data/fundamentals] [--from-year 2018]
单位铁律: roe/np_yoy/rev_yoy/gross_margin = 百分数(原样), eps/bps = 元。time = 最新公告日。"""
import argparse, os, sys
import akshare as ak
import pandas as pd

COLMAP = {
    "净资产收益率": "roe",
    "净利润-同比增长": "np_yoy",
    "营业总收入-同比增长": "rev_yoy",
    "销售毛利率": "gross_margin",
    "每股收益": "eps",
    "每股净资产": "bps",
}
OUT_COLS = ["roe", "np_yoy", "rev_yoy", "gross_margin", "eps", "bps"]

def to_symbol(code: str):
    code = str(code).zfill(6)
    if code[:2] in ("60", "68") or code[0] == "9":
        return "sh" + code
    if code[:2] in ("00", "30") or code[0] == "2":
        return "sz" + code
    return None

def quarters(from_year: int):
    import datetime
    today = datetime.date.today()
    out = []
    for y in range(from_year, today.year + 1):
        for md in ("0331", "0630", "0930", "1231"):
            d = f"{y}{md}"
            qend = datetime.date(int(d[:4]), int(d[4:6]), int(d[6:8]))
            if qend < today:
                out.append(d)
    return out

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="data/fundamentals")
    ap.add_argument("--from-year", type=int, default=2018)
    args = ap.parse_args()
    os.makedirs(args.out, exist_ok=True)
    per_stock = {}
    for q in quarters(args.from_year):
        try:
            df = ak.stock_yjbb_em(date=q)
        except Exception as e:
            print(f"WARN quarter {q} failed: {e}", file=sys.stderr); continue
        for _, r in df.iterrows():
            sym = to_symbol(r.get("股票代码", ""))
            if sym is None:
                continue
            ann = r.get("最新公告日期")
            if pd.isna(ann):
                continue
            ann = str(ann)[:10]
            row = {}
            for zh, en in COLMAP.items():
                v = r.get(zh)
                row[en] = "" if pd.isna(v) else f"{float(v):.6g}"
            per_stock.setdefault(sym, {})[ann] = row
        print(f"  quarter {q}: {df.shape[0]} rows", file=sys.stderr)
    n = 0
    for sym, byann in per_stock.items():
        rows = sorted(byann.items())
        path = os.path.join(args.out, f"{sym}.csv")
        with open(path, "w", encoding="utf-8", newline="") as f:
            f.write("time," + ",".join(OUT_COLS) + "\n")
            for ann, row in rows:
                f.write(ann + "," + ",".join(row[c] for c in OUT_COLS) + "\n")
        n += 1
    print(f"wrote {n} per-stock fundamental CSVs to {args.out}")

if __name__ == "__main__":
    main()
