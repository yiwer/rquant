"""导出周频因子面板：40 精选因子 + 未来5日收益。PIT + membership 点时掩码。
产出 data/factor_panel/factors.csv（行=(date,symbol)）。
--no-membership 模式跳过成分掩码，写 data/factor_panel/factors_full.csv。"""
import sys
sys.stdout.reconfigure(encoding="utf-8")
import argparse
import os
import numpy as np
import pandas as pd

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
KDAY = os.path.join(REPO, "data", "baostock", "kday")
FUND = os.path.join(REPO, "data", "fundamentals")
SEC = os.path.join(REPO, "data", "baostock", "pa_sector_merged")
ROSTER = os.path.join(REPO, "data", "baostock", "universe_baostock_day.csv")
MEMBERSHIP = os.path.join(REPO, "data", "membership_top2000.csv")
OUT_DIR = os.path.join(REPO, "data", "factor_panel")
OUT = os.path.join(OUT_DIR, "factors.csv")
HOLD = 5            # 周频持有期（交易日）
FROM, TO = "2018-01-01", "2026-06-30"

FACTOR_COLS = [
    # --- existing 13 (indices 0-12; f_bm@0, f_npyoy@1 must stay) ---
    "f_bm", "f_npyoy", "f_revyoy", "f_roe", "f_gm",
    "f_mom20", "f_mom120", "f_rev5", "f_trend60",
    "f_atr", "f_rvol", "f_logamt", "f_secmom",
    # --- new 24 (indices 13-36) ---
    # price/momentum
    "f_mom60", "f_mom250", "f_rev10", "f_trend20",
    "f_hi52", "f_donch60",
    # technical indicators
    "f_rsi14", "f_macd", "f_bollpctb",
    # volatility
    "f_vol20", "f_volratio", "f_maxdd60",
    # volume/liquidity
    "f_turn", "f_turnmean", "f_amihud", "f_voltrend",
    # fundamental
    "f_ep",
    # sector/PA signals
    "f_padir", "f_pastruct", "f_paregime", "f_papull",
    "f_sectrend", "f_secbreadth", "f_secheat",
    # --- new 3 price factors (indices 37-39) ---
    "f_maxret20",   # 20-day rolling max daily return (lottery/MAX anomaly)
    "f_skew60",     # 60-day rolling skewness of daily returns
    "f_relstr60",   # 60-day relative strength vs CSI300 (beta-stripped momentum)
]


def atr14(high, low, close, n=14):
    """Wilder ATR。第 1 根因无前收盘 TR 为 NaN。当数据足够（>n 根）使用 Wilder 平滑；
    数据不足时退化为可用 TR 的 nanmean，保证有效数据点≥2 时末尾返回非 NaN。"""
    high = np.asarray(high, float)
    low = np.asarray(low, float)
    close = np.asarray(close, float)
    prev = np.concatenate([[np.nan], close[:-1]])
    tr = np.maximum(
        high - low,
        np.maximum(np.abs(high - prev), np.abs(low - prev))
    )
    atr = np.full(len(tr), np.nan)
    if len(tr) > n:
        # 正常路径：首个 ATR = tr[1..n] 均值，之后 Wilder 平滑
        atr[n] = np.nanmean(tr[1:n + 1])
        for i in range(n + 1, len(tr)):
            atr[i] = (atr[i - 1] * (n - 1) + tr[i]) / n
    else:
        # 数据不足路径：用所有有效 TR 均值填满最后一位（至少需要 1 根有效 TR）
        valid = tr[1:]   # tr[0] 无前收（NaN），从 index 1 开始
        if len(valid) >= 1:
            mean_tr = float(np.nanmean(valid))
            if not np.isnan(mean_tr):
                atr[-1] = mean_tr
    return atr


def compute_symbol_factors(kday, fund, sec, index_close=None):
    """Compute per-day factors + 5-day forward return for one symbol.

    Args:
        kday: DataFrame with columns time, open, high, low, close, volume,
              amount, turn, pctChg (ascending by time).
        fund: DataFrame with time-point fundamentals (PIT, forward-filled via merge_asof).
        sec:  DataFrame with columns date, sec_mom20 (and optionally pa_dir,
              pa_struct, pa_regime, pa_pullback, sec_trend, sec_breadth, sec_heat),
              or None.
        index_close: optional pd.Series mapping date string (YYYY-MM-DD) -> float,
              representing the CSI300 daily close. Used to compute f_relstr60.
              When None, f_relstr60 is all-NaN.
    """
    # Sort by time first so all positional .values assignments are guaranteed aligned,
    # regardless of whether the caller supplied an already-sorted frame.
    df = kday.sort_values("time").reset_index(drop=True)
    c = df["close"].astype(float)
    v = df["volume"].astype(float)

    # Build output frame; keep original string dates for the index
    out = pd.DataFrame({"date": df["time"].values})

    # ---- Fundamentals: PIT forward-fill via merge_asof on datetime keys ----
    f = fund.sort_values("time").copy()
    # Convert to datetime for reliable merge_asof comparison
    kday_times = pd.to_datetime(df["time"])
    fund_times = pd.to_datetime(f["time"])

    fmap = pd.DataFrame({"time_dt": kday_times})
    f2 = f.copy()
    f2["time_dt"] = fund_times
    # merge_asof requires both keys sorted and same type
    fmap = fmap.sort_values("time_dt")
    f2 = f2.sort_values("time_dt")
    fmap = pd.merge_asof(fmap, f2, on="time_dt", direction="backward")
    # fmap is now aligned to the sorted df index; positional .values is safe

    out["f_bm"] = fmap["bps"].values / c.values
    out["f_npyoy"] = fmap["np_yoy"].values
    out["f_revyoy"] = fmap["rev_yoy"].values
    out["f_roe"] = fmap["roe"].values
    out["f_gm"] = fmap["gross_margin"].values

    # ---- Price-based factors (existing) ----
    out["f_mom20"] = (c / c.shift(20) - 1).values
    out["f_mom120"] = (c / c.shift(120) - 1).values
    out["f_rev5"] = (c / c.shift(5) - 1).values
    out["f_trend60"] = (c / c.rolling(60).mean() - 1).values
    out["f_atr"] = atr14(df["high"].astype(float), df["low"].astype(float), c) / c.values
    out["f_rvol"] = (v / v.rolling(20).mean()).values
    amt = (c * v).rolling(20).mean()
    out["f_logamt"] = np.log(amt.where(amt > 0)).values

    # ---- Sector momentum (existing) ----
    if sec is not None and len(sec) > 0:
        # sec has columns: date, sec_mom20 (or time, sec_mom20)
        s = sec.copy()
        # Normalize column name: accept 'time' or 'date'
        if "time" in s.columns and "date" not in s.columns:
            s = s.rename(columns={"time": "date"})
        # Normalize dates to YYYY-MM-DD for merge
        s["date"] = s["date"].astype(str).str[:10]
        out["date"] = out["date"].astype(str).str[:10]

        # sec_mom20 -> f_secmom
        sec_cols_available = [col for col in
                              ["sec_mom20", "pa_dir", "pa_struct", "pa_regime",
                               "pa_pullback", "sec_trend", "sec_breadth", "sec_heat"]
                              if col in s.columns]
        s_sub = s[["date"] + sec_cols_available].copy()
        out = out.merge(s_sub, on="date", how="left")

        # Map raw column names to factor names
        if "sec_mom20" in s_sub.columns:
            out = out.rename(columns={"sec_mom20": "f_secmom"})
        else:
            out["f_secmom"] = np.nan
    else:
        out["date"] = out["date"].astype(str).str[:10]
        out["f_secmom"] = np.nan

    # Fill PA/sector factors that may be missing
    _pa_map = {
        "pa_dir": "f_padir", "pa_struct": "f_pastruct", "pa_regime": "f_paregime",
        "pa_pullback": "f_papull", "sec_trend": "f_sectrend",
        "sec_breadth": "f_secbreadth", "sec_heat": "f_secheat",
    }
    for raw, fname in _pa_map.items():
        if raw in out.columns:
            out = out.rename(columns={raw: fname})
        else:
            out[fname] = np.nan

    # ---- NEW price/momentum factors ----
    h = df["high"].astype(float)
    lo = df["low"].astype(float)

    out["f_mom60"] = (c / c.shift(60) - 1).values
    out["f_mom250"] = (c / c.shift(250) - 1).values
    out["f_rev10"] = (c / c.shift(10) - 1).values
    out["f_trend20"] = (c / c.rolling(20).mean() - 1).values
    out["f_hi52"] = (c / c.rolling(250).max()).values
    donch_range = (h.rolling(60).max() - lo.rolling(60).min()).replace(0, np.nan)
    out["f_donch60"] = ((c - lo.rolling(60).min()) / donch_range).values

    # ---- RSI-14 (Wilder) ----
    d = c.diff()
    up = d.clip(lower=0).ewm(alpha=1/14, adjust=False).mean()
    dn = (-d.clip(upper=0)).ewm(alpha=1/14, adjust=False).mean()
    # Guard: when dn==0 (pure uptrend) RSI=100; use np.where to avoid NaN from 0-division
    dn_safe = dn.where(dn.abs() > 0)  # NaN when dn is ±0
    rsi = 100 - 100 / (1 + up / dn_safe)
    # When dn_safe is NaN and up>0 → RSI=100 (all up); when both 0 → RSI=50 (no movement)
    rsi = rsi.where(dn_safe.notna(), other=np.where(up.values > 0, 100.0, 50.0))
    out["f_rsi14"] = rsi.values

    # ---- MACD histogram (normalised by close) ----
    e12 = c.ewm(span=12, adjust=False).mean()
    e26 = c.ewm(span=26, adjust=False).mean()
    dif = e12 - e26
    hist = dif - dif.ewm(span=9, adjust=False).mean()
    out["f_macd"] = (hist / c).values

    # ---- Bollinger %B ----
    ma20 = c.rolling(20).mean()
    sd20 = c.rolling(20).std()
    out["f_bollpctb"] = ((c - (ma20 - 2 * sd20)) / (4 * sd20).replace(0, np.nan)).values

    # ---- Volatility ----
    ret = c.pct_change()
    vol20 = ret.rolling(20).std()
    vol60 = ret.rolling(60).std()
    out["f_vol20"] = vol20.values
    out["f_volratio"] = (vol20 / vol60.replace(0, np.nan)).values
    out["f_maxdd60"] = (1 - c / c.rolling(60).max()).values

    # ---- Volume/liquidity ----
    out["f_turn"] = df["turn"].values
    out["f_turnmean"] = df["turn"].rolling(20).mean().values
    pct_abs = df["pctChg"].abs()
    amt_col = df["amount"].where(df["amount"] > 0)
    out["f_amihud"] = (pct_abs / amt_col).rolling(20).mean().values
    out["f_voltrend"] = (v.rolling(5).mean() / v.rolling(60).mean() - 1).values

    # ---- Fundamental: earnings yield ----
    out["f_ep"] = (fmap["eps"].values / c.values)

    # ---- New price factors (indices 37-39) ----
    r = c.pct_change()

    # f_maxret20: 20-day rolling max of daily return (lottery/MAX anomaly)
    out["f_maxret20"] = r.rolling(20).max().values

    # f_skew60: 60-day rolling skewness of daily returns
    out["f_skew60"] = r.rolling(60).skew().values

    # f_relstr60: relative strength vs CSI300 over 60 days (beta-stripped momentum)
    # PIT: only uses past data (shift(60) looks 60 days back; no future leakage)
    if index_close is not None:
        # Align CSI300 close to this symbol's date axis via map (forward-fill: ffill after map)
        date_col = out["date"].astype(str).str[:10]
        idx_aligned = date_col.map(index_close)
        # Forward-fill gaps (e.g. minor calendar mismatches) within symbol's date range
        idx_aligned = idx_aligned.astype(float).ffill()
        sym_ret60 = c / c.shift(60) - 1
        idx_ret60 = idx_aligned / idx_aligned.shift(60) - 1
        out["f_relstr60"] = (sym_ret60 - idx_ret60).values
    else:
        out["f_relstr60"] = np.nan

    # ---- Forward return label ----
    out["fwd_ret_5d"] = (c.shift(-HOLD) / c - 1).values

    return out.set_index("date")


def _weekly_dates(all_dates):
    """全交易日并集升序，每 HOLD 个取一个调仓日。"""
    ds = sorted(set(all_dates))
    return ds[::HOLD]


def mask_by_membership(panel_df, members_at):
    """Filter panel rows to those whose symbol ∈ members_at(date).

    Args:
        panel_df: DataFrame with columns 'date' and 'symbol'.
        members_at: callable(date_str) -> set of member symbols.

    Returns:
        Filtered DataFrame (subset of rows), index preserved.
    """
    keep = [sym in members_at(d) for d, sym in zip(panel_df["date"], panel_df["symbol"])]
    return panel_df[keep]


def _load_index_close(csv_path):
    """Load CSI300 daily close from data/baostock/index/csi300.csv.

    Expected columns: time (YYYY-MM-DD HH:MM:SS or YYYY-MM-DD), close.
    Returns pd.Series mapping date string YYYY-MM-DD -> float close, or None if file missing.
    """
    if not os.path.exists(csv_path):
        return None
    df = pd.read_csv(csv_path)
    df["date"] = df["time"].astype(str).str[:10]
    df["close"] = df["close"].astype(float)
    # Keep last entry per date (in case of duplicates)
    df = df.sort_values("date").drop_duplicates("date", keep="last")
    return df.set_index("date")["close"]


def main(apply_membership=True, out_path=OUT):
    os.makedirs(OUT_DIR, exist_ok=True)
    roster = pd.read_csv(ROSTER)["symbol"].tolist()

    # Load CSI300 index close once for f_relstr60
    csi300_path = os.path.join(REPO, "data", "baostock", "index", "csi300.csv")
    index_close = _load_index_close(csi300_path)

    # Only load membership data when the mask is actually needed.
    if apply_membership:
        mem = pd.read_csv(MEMBERSHIP)                   # date,symbol（月末快照）

        # Pre-compute membership snapshot sets keyed by snapshot date (string).
        # This avoids rebuilding sets inside the hot loop (was O(syms × rebs) set creations).
        mem_dates_sorted = sorted(mem["date"].unique())
        mem_snap = {d: set(mem[mem["date"] == d]["symbol"]) for d in mem_dates_sorted}

        def members_at_cached(d_str):
            """≤d 最近快照的成分集（从预计算字典取，O(log n) 查找 + O(1) 返回）。"""
            # d_str may be a string "YYYY-MM-DD" or "YYYY-MM-DD HH:MM:SS"
            d_key = str(d_str)[:10]   # keep only date part for comparison
            i = np.searchsorted(mem_dates_sorted, d_key, side="right") - 1
            if i < 0:
                return set()
            return mem_snap[mem_dates_sorted[i]]

    frames, all_dates = {}, set()
    for sym in roster:
        kp = os.path.join(KDAY, f"{sym}.csv")
        fp = os.path.join(FUND, f"{sym}.csv")
        if not (os.path.exists(kp) and os.path.exists(fp)):
            continue
        kday = pd.read_csv(kp)
        fund = pd.read_csv(fp)
        kday = kday[(kday["time"] >= FROM) & (kday["time"] <= TO)]
        if len(kday) < 130:
            continue
        sp = os.path.join(SEC, f"{sym}.csv")
        sec = None
        if os.path.exists(sp):
            sec = pd.read_csv(sp)[
                ["time", "sec_mom20", "pa_dir", "pa_struct", "pa_regime",
                 "pa_pullback", "sec_trend", "sec_breadth", "sec_heat"]
            ].rename(columns={"time": "date"})
        fac = compute_symbol_factors(kday, fund, sec, index_close=index_close)
        frames[sym] = fac
        all_dates.update(fac.index)

    rebs = _weekly_dates(all_dates)
    # Rebalance dates as plain date strings for membership lookup
    rebs_str = [str(r)[:10] for r in rebs]
    rebs_set = set(rebs_str)

    rows = []
    for sym, fac in frames.items():
        # Align fac index (may be string or datetime) to rebalance date strings
        fac_dates_str = [str(d)[:10] for d in fac.index]
        # Keep only rebalance-date rows
        reb_mask = np.array([d in rebs_set for d in fac_dates_str])
        if not reb_mask.any():
            continue
        sub = fac.loc[np.array(fac.index)[reb_mask], FACTOR_COLS + ["fwd_ret_5d"]].copy()
        sub.insert(0, "symbol", sym)
        sub.insert(0, "date", [str(d)[:10] for d in sub.index])
        rows.append(sub)

    panel = pd.concat(rows, ignore_index=True).sort_values(["date", "symbol"])

    # Apply point-in-time membership mask when requested
    if apply_membership:
        panel = mask_by_membership(panel, members_at_cached)

    panel.to_csv(out_path, index=False, encoding="utf-8")
    print(
        f"wrote {len(panel)} rows x {len(FACTOR_COLS)} factors -> {out_path}"
        f"  (dates {panel['date'].min()}..{panel['date'].max()})"
    )


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Build weekly factor panel.")
    parser.add_argument(
        "--no-membership",
        action="store_true",
        help="Skip membership mask; write factors_full.csv instead of factors.csv.",
    )
    args = parser.parse_args()

    if args.no_membership:
        out_path = OUT.replace("factors.csv", "factors_full.csv")
        main(apply_membership=False, out_path=out_path)
    else:
        main()
