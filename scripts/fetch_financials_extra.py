"""Fetch ORTHOGONAL quarterly financial factors (cash flow / leverage / capital efficiency)
via akshare stock_financial_abstract — one CSV per stock, point-in-time keyed.

Usage:
    python scripts/fetch_financials_extra.py [--out data/financials_extra] [--from-year 2018]

Resumable: skips symbols whose CSV already exists.
"""
import argparse
import os
import sys
import time

import akshare as ak
import pandas as pd

# Ordered list of (指标 name, output column).  FIRST matching row is used.
INDICATOR_MAP = [
    ("经营现金流量净额",               "cfo"),
    ("经营活动净现金/归属母公司的净利润", "cfo_to_np"),
    ("经营性现金净流量/营业总收入",      "cfo_to_rev"),
    ("资产负债率",                     "debt_ratio"),
    ("投入资本回报率",                  "roic"),
    ("总资产报酬率(ROA)",               "roa"),
    ("销售净利率",                     "net_margin"),
    ("营业利润率",                     "op_margin"),
    ("流动比率",                       "current_ratio"),
    ("速动比率",                       "quick_ratio"),
    ("现金比率",                       "cash_ratio"),
    ("权益乘数",                       "equity_mult"),
    ("总资产周转率",                    "asset_turn"),
    ("存货周转率",                      "inv_turn"),
    ("应收账款周转率",                  "ar_turn"),
]

OUT_COLS = [col for _, col in INDICATOR_MAP]


def period_to_disclosure(period: str) -> str:
    """Map report-period-end YYYYMMDD → conservative A-share disclosure deadline (ISO date).

    Rules (no lookahead):
      MMDD 0331 → same-year  Apr 30  "{Y}-04-30"
      MMDD 0630 → same-year  Aug 31  "{Y}-08-31"
      MMDD 0930 → same-year  Oct 31  "{Y}-10-31"
      MMDD 1231 → next-year  Apr 30  "{Y+1}-04-30"
    """
    y = int(period[:4])
    mmdd = period[4:]
    if mmdd == "0331":
        return f"{y}-04-30"
    elif mmdd == "0630":
        return f"{y}-08-31"
    elif mmdd == "0930":
        return f"{y}-10-31"
    elif mmdd == "1231":
        return f"{y + 1}-04-30"
    else:
        raise ValueError(f"Unknown period suffix: {mmdd!r} in {period!r}")


def extract_series(fa_df: pd.DataFrame, from_year: int = 2018) -> dict:
    """Extract indicator series from stock_financial_abstract DataFrame.

    Returns: {disclosure_date_str -> {col_name -> value_str_or_float}}
    Period columns with year < from_year are skipped.
    For each indicator the FIRST row whose 指标 matches is used.
    Numeric values are coerced to float; blank/NaN → "".
    """
    # Identify period-end columns: all non-指标 columns matching 8-digit YYYYMMDD
    import re
    period_cols = [c for c in fa_df.columns
                   if re.fullmatch(r"\d{8}", str(c)) and int(str(c)[:4]) >= from_year]

    # Build lookup: 指标 → first matching row index
    indicator_index: dict[str, int] = {}
    for zh, _ in INDICATOR_MAP:
        if zh in indicator_index:
            continue
        mask = fa_df["指标"] == zh
        if mask.any():
            indicator_index[zh] = fa_df.index[mask][0]

    result: dict = {}
    for period in period_cols:
        disclosure = period_to_disclosure(str(period))
        row_data: dict = {}
        for zh, col in INDICATOR_MAP:
            idx = indicator_index.get(zh)
            if idx is None:
                row_data[col] = ""
                continue
            raw = fa_df.at[idx, period]
            if raw is None or (isinstance(raw, float) and pd.isna(raw)):
                row_data[col] = ""
            else:
                try:
                    row_data[col] = float(raw)
                except (TypeError, ValueError):
                    row_data[col] = "" if str(raw).strip() == "" else str(raw).strip()
        result[disclosure] = row_data

    return result


def to_symbol(code: str) -> str | None:
    """Mirror fetch_fundamentals.py: 6-digit code → sh/sz-prefixed symbol."""
    code = str(code).zfill(6)
    if code[:2] in ("60", "68") or code[0] == "9":
        return "sh" + code
    if code[:2] in ("00", "30") or code[0] == "2":
        return "sz" + code
    return None


def _format_value(v) -> str:
    if v == "" or v is None:
        return ""
    if isinstance(v, float):
        if pd.isna(v):
            return ""
        return f"{v:.6g}"
    return str(v)


def main():
    ap = argparse.ArgumentParser(description="Fetch orthogonal financial factors per stock.")
    ap.add_argument("--out", default="data/financials_extra")
    ap.add_argument("--from-year", type=int, default=2018)
    args = ap.parse_args()

    os.makedirs(args.out, exist_ok=True)

    # Load universe roster
    roster_path = "data/baostock/universe_baostock_day.csv"
    roster_df = pd.read_csv(roster_path, usecols=["symbol"])
    symbols = roster_df["symbol"].dropna().unique().tolist()
    print(f"Roster loaded: {len(symbols)} symbols", file=sys.stderr)

    header = "time," + ",".join(OUT_COLS) + "\n"

    done = 0
    skipped = 0
    warned = 0

    for i, sym in enumerate(symbols, 1):
        out_path = os.path.join(args.out, f"{sym}.csv")

        # Resumable: skip if file already exists
        if os.path.exists(out_path):
            skipped += 1
            continue

        # Derive 6-digit code for akshare
        code6 = sym[2:]  # strip sh/sz prefix

        try:
            fa_df = ak.stock_financial_abstract(symbol=code6)
            if fa_df is None or fa_df.empty:
                print(f"WARN {sym}: empty DataFrame returned", file=sys.stderr)
                warned += 1
                continue

            series = extract_series(fa_df, from_year=args.from_year)
            rows = sorted(series.items())  # sort by disclosure date

            with open(out_path, "w", encoding="utf-8", newline="") as f:
                f.write(header)
                for disclosure, row_data in rows:
                    vals = ",".join(_format_value(row_data.get(c, "")) for c in OUT_COLS)
                    f.write(f"{disclosure},{vals}\n")

            done += 1

        except Exception as e:
            print(f"WARN {sym}: {e}", file=sys.stderr)
            warned += 1
            continue

        # Progress report every 50 symbols
        if i % 50 == 0:
            print(f"  [{i}/{len(symbols)}] done={done} skipped={skipped} warned={warned}",
                  file=sys.stderr)

        time.sleep(0.3)

    print(
        f"Finished. wrote={done} skipped={skipped} warned={warned} "
        f"total_symbols={len(symbols)}",
        file=sys.stderr,
    )


if __name__ == "__main__":
    main()
