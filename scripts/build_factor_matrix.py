"""导出周频因子面板：13 精选因子 + 未来5日收益。PIT + membership 点时掩码。
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
    "f_bm", "f_npyoy", "f_revyoy", "f_roe", "f_gm",
    "f_mom20", "f_mom120", "f_rev5", "f_trend60",
    "f_atr", "f_rvol", "f_logamt", "f_secmom",
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


def compute_symbol_factors(kday, fund, sec):
    """Compute per-day factors + 5-day forward return for one symbol.

    Args:
        kday: DataFrame with columns time, open, high, low, close, volume (ascending by time).
        fund: DataFrame with time-point fundamentals (PIT, forward-filled via merge_asof).
        sec:  DataFrame with columns date, sec_mom20 (sector momentum), or None.
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

    # ---- Price-based factors ----
    out["f_mom20"] = (c / c.shift(20) - 1).values
    out["f_mom120"] = (c / c.shift(120) - 1).values
    out["f_rev5"] = (c / c.shift(5) - 1).values
    out["f_trend60"] = (c / c.rolling(60).mean() - 1).values
    out["f_atr"] = atr14(df["high"].astype(float), df["low"].astype(float), c) / c.values
    out["f_rvol"] = (v / v.rolling(20).mean()).values
    amt = (c * v).rolling(20).mean()
    out["f_logamt"] = np.log(amt.where(amt > 0)).values

    # ---- Sector momentum ----
    if sec is not None and len(sec) > 0:
        # sec has columns: date, sec_mom20 (or time, sec_mom20)
        s = sec.copy()
        # Normalize column name: accept 'time' or 'date'
        if "time" in s.columns and "date" not in s.columns:
            s = s.rename(columns={"time": "date"})
        s = s.rename(columns={"sec_mom20": "f_secmom"})[["date", "f_secmom"]]
        # Normalize dates to YYYY-MM-DD for merge: kday times may carry HH:MM:SS suffix
        s["date"] = s["date"].astype(str).str[:10]
        out["date"] = out["date"].astype(str).str[:10]
        out = out.merge(s, on="date", how="left")
    else:
        out["f_secmom"] = np.nan

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


def main(apply_membership=True, out_path=OUT):
    os.makedirs(OUT_DIR, exist_ok=True)
    roster = pd.read_csv(ROSTER)["symbol"].tolist()

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
            sec = pd.read_csv(sp)[["time", "sec_mom20"]].rename(columns={"time": "date"})
        fac = compute_symbol_factors(kday, fund, sec)
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
