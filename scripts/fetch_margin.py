"""Fetch per-stock daily 融资融券 (margin-trading) data via akshare.

Data is fetched per DATE across all stocks (SSE + SZSE), cached in
data/margin_raw/, then pivoted into per-symbol CSVs under data/margin/.

Usage:
    python scripts/fetch_margin.py [--from-date 2018-01-01] [--out data/margin]
    python scripts/fetch_margin.py --pivot-only   # rebuild per-symbol from cache

Resumable: per-date raw cache files are skipped if both sse_ and szse_ files
already exist for that date.
"""
import argparse
import os
import sys
import time

import pandas as pd


# ---------------------------------------------------------------------------
# Pure helper functions (tested in test_fetch_margin.py, no network)
# ---------------------------------------------------------------------------

def to_symbol_sse(code) -> str:
    """6-digit SSE code → sh-prefixed symbol string.

    Args:
        code: str or int, may be shorter than 6 digits (zero-padded).
    Returns:
        "sh{6-digit-code}", e.g. "sh600519"
    """
    return "sh" + str(code).zfill(6)


def to_symbol_szse(code) -> str:
    """6-digit SZSE code → sz-prefixed symbol string.

    Args:
        code: str or int, may be shorter than 6 digits (zero-padded).
    Returns:
        "sz{6-digit-code}", e.g. "sz000001"
    """
    return "sz" + str(code).zfill(6)


def _norm_date(raw) -> str:
    """Normalize a date value to YYYY-MM-DD string.

    Accepts:
      - str "20180102"  → "2018-01-02"
      - str "2018-01-02" → unchanged
      - pandas Timestamp / datetime → .strftime("%Y-%m-%d")
    """
    s = str(raw).strip()
    # Remove time component if present (e.g. "2018-01-02 00:00:00")
    s = s.split(" ")[0]
    if len(s) == 8 and s.isdigit():
        return f"{s[:4]}-{s[4:6]}-{s[6:8]}"
    return s  # already YYYY-MM-DD or passthrough


def normalize_sse(df: pd.DataFrame) -> pd.DataFrame:
    """Normalize raw ak.stock_margin_detail_sse DataFrame to canonical schema.

    SSE columns (by name, position-independent):
        标的证券代码, 信用交易日期, 融资余额, 融资买入额, 融券余量  (and others)

    Returns:
        DataFrame with columns [symbol, date, rzye, rzmre, rqyl]
        - symbol: sh-prefixed 6-digit code
        - date:   "YYYY-MM-DD"
        - rzye:   float, 融资余额
        - rzmre:  float, 融资买入额
        - rqyl:   float, 融券余量
    """
    out = pd.DataFrame()
    out["symbol"] = df["标的证券代码"].apply(to_symbol_sse)
    out["date"] = df["信用交易日期"].apply(_norm_date)
    out["rzye"] = pd.to_numeric(df["融资余额"], errors="coerce")
    out["rzmre"] = pd.to_numeric(df["融资买入额"], errors="coerce")
    out["rqyl"] = pd.to_numeric(df["融券余量"], errors="coerce")
    return out.reset_index(drop=True)


def normalize_szse(df: pd.DataFrame, date_yyyymmdd: str) -> pd.DataFrame:
    """Normalize raw ak.stock_margin_detail_szse DataFrame to canonical schema.

    SZSE columns (by name, position-independent; NO date column in data):
        证券代码, 证券简称, 融资买入额, 融资余额, 融券卖出量, 融券余量, 融券余额, 融资融券余额

    Args:
        df:              Raw DataFrame from akshare.
        date_yyyymmdd:  The query date string e.g. "20180102" (used as date).

    Returns:
        DataFrame with columns [symbol, date, rzye, rzmre, rqyl]
    """
    date_iso = _norm_date(date_yyyymmdd)
    out = pd.DataFrame()
    out["symbol"] = df["证券代码"].apply(to_symbol_szse)
    out["date"] = date_iso
    out["rzye"] = pd.to_numeric(df["融资余额"], errors="coerce")
    out["rzmre"] = pd.to_numeric(df["融资买入额"], errors="coerce")
    out["rqyl"] = pd.to_numeric(df["融券余量"], errors="coerce")
    return out.reset_index(drop=True)


def pivot_to_symbol(rows_df: pd.DataFrame) -> dict:
    """Pivot a concatenated rows DataFrame to per-symbol time-series DataFrames.

    Args:
        rows_df: DataFrame with columns [symbol, date, rzye, rzmre, rqyl]
                 (may contain rows from many dates and both exchanges)

    Returns:
        dict {symbol: DataFrame[time, rzye, rzmre, rqyl]}
        - Each DataFrame is sorted ascending by time.
        - Duplicate time values are deduped (keep last occurrence).
        - Column is named "time" to match other per-symbol CSVs in the project.
    """
    result = {}
    if rows_df.empty:
        return result

    for sym, grp in rows_df.groupby("symbol"):
        # Rename date → time for project consistency
        sym_df = grp[["date", "rzye", "rzmre", "rqyl"]].copy()
        sym_df = sym_df.rename(columns={"date": "time"})
        # Dedup on time keeping last
        sym_df = sym_df.drop_duplicates(subset=["time"], keep="last")
        # Sort ascending
        sym_df = sym_df.sort_values("time").reset_index(drop=True)
        result[sym] = sym_df

    return result


# ---------------------------------------------------------------------------
# Fetch driver (networked — NOT called by tests)
# ---------------------------------------------------------------------------

def _trading_days(from_date: str = "2018-01-01") -> list[str]:
    """Load unique trading dates from baostock CSI300 index file.

    Returns:
        Sorted list of "YYYYMMDD" strings >= from_date.
    """
    idx_path = "data/baostock/index/csi300.csv"
    df = pd.read_csv(idx_path, usecols=["time"])
    dates = df["time"].dropna().astype(str).str[:10].unique()
    dates = sorted(d for d in dates if d >= from_date)
    return [d.replace("-", "") for d in dates]  # → YYYYMMDD for akshare


def _raw_cache_paths(raw_dir: str, yyyymmdd: str):
    sse_path = os.path.join(raw_dir, f"sse_{yyyymmdd}.csv")
    szse_path = os.path.join(raw_dir, f"szse_{yyyymmdd}.csv")
    return sse_path, szse_path


def _fetch_date(yyyymmdd: str, raw_dir: str) -> tuple[int, int]:
    """Fetch SSE + SZSE margin data for one date and write to raw cache.

    Returns:
        (n_sse_rows, n_szse_rows); each is 0 on error.
    Skips if both cache files already exist.
    """
    import akshare as ak

    sse_path, szse_path = _raw_cache_paths(raw_dir, yyyymmdd)
    if os.path.exists(sse_path) and os.path.exists(szse_path):
        return (-1, -1)  # sentinel: skipped

    n_sse = 0
    n_szse = 0

    # SSE
    try:
        df_sse = ak.stock_margin_detail_sse(date=yyyymmdd)
        if df_sse is not None and not df_sse.empty:
            norm = normalize_sse(df_sse)
            norm.to_csv(sse_path, index=False)
            n_sse = len(norm)
        else:
            # Write empty file so we don't refetch
            pd.DataFrame(columns=["symbol", "date", "rzye", "rzmre", "rqyl"]).to_csv(
                sse_path, index=False
            )
    except Exception as e:
        print(f"WARN sse {yyyymmdd}: {e}", file=sys.stderr)

    time.sleep(0.3)

    # SZSE
    try:
        df_szse = ak.stock_margin_detail_szse(date=yyyymmdd)
        if df_szse is not None and not df_szse.empty:
            norm = normalize_szse(df_szse, yyyymmdd)
            norm.to_csv(szse_path, index=False)
            n_szse = len(norm)
        else:
            pd.DataFrame(columns=["symbol", "date", "rzye", "rzmre", "rqyl"]).to_csv(
                szse_path, index=False
            )
    except Exception as e:
        print(f"WARN szse {yyyymmdd}: {e}", file=sys.stderr)

    time.sleep(0.3)

    return (n_sse, n_szse)


def _load_all_cache(raw_dir: str) -> pd.DataFrame:
    """Read all cache CSV files under raw_dir and concatenate into one DataFrame."""
    parts = []
    for fname in sorted(os.listdir(raw_dir)):
        if not fname.endswith(".csv"):
            continue
        fpath = os.path.join(raw_dir, fname)
        try:
            df = pd.read_csv(fpath)
            if not df.empty:
                parts.append(df)
        except Exception as e:
            print(f"WARN loading {fpath}: {e}", file=sys.stderr)
    if not parts:
        return pd.DataFrame(columns=["symbol", "date", "rzye", "rzmre", "rqyl"])
    return pd.concat(parts, ignore_index=True)


def main():
    ap = argparse.ArgumentParser(description="Fetch per-stock daily margin-trading data.")
    ap.add_argument("--from-date", default="2018-01-01",
                    help="Start date YYYY-MM-DD (default 2018-01-01)")
    ap.add_argument("--out", default="data/margin",
                    help="Directory for per-symbol output CSVs")
    ap.add_argument("--pivot-only", action="store_true",
                    help="Skip fetch; just rebuild per-symbol CSVs from existing raw cache")
    args = ap.parse_args()

    raw_dir = "data/margin_raw"
    os.makedirs(raw_dir, exist_ok=True)
    os.makedirs(args.out, exist_ok=True)

    n_fetched = 0
    n_skipped = 0

    if not args.pivot_only:
        days = _trading_days(from_date=args.from_date)
        print(f"Trading days to process: {len(days)} (from {args.from_date})", file=sys.stderr)

        for i, yyyymmdd in enumerate(days, 1):
            n_sse, n_szse = _fetch_date(yyyymmdd, raw_dir)
            if n_sse == -1:
                n_skipped += 1
            else:
                n_fetched += 1

            if i % 50 == 0:
                print(
                    f"  [{i}/{len(days)}] fetched={n_fetched} skipped={n_skipped}",
                    file=sys.stderr,
                )

        print(
            f"Fetch done. dates_fetched={n_fetched} dates_skipped={n_skipped}",
            file=sys.stderr,
        )

    # Pivot phase: read all cache → per-symbol CSVs
    print("Pivoting raw cache to per-symbol CSVs ...", file=sys.stderr)
    all_rows = _load_all_cache(raw_dir)
    sym_map = pivot_to_symbol(all_rows)

    n_written = 0
    for sym, sym_df in sym_map.items():
        out_path = os.path.join(args.out, f"{sym}.csv")
        sym_df.to_csv(out_path, index=False)
        n_written += 1

    print(
        f"Pivot done. symbols_written={n_written}",
        file=sys.stderr,
    )
    print(
        f"Summary: dates_fetched={n_fetched} dates_skipped={n_skipped} "
        f"symbols_written={n_written}",
        file=sys.stderr,
    )


if __name__ == "__main__":
    main()
