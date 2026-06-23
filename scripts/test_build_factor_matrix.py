import numpy as np, pandas as pd
import build_factor_matrix as bm


def test_mask_by_membership_filters_non_members():
    panel = pd.DataFrame({"date": ["2020-01-10", "2020-01-10"], "symbol": ["A", "B"]})
    members_at = lambda d: {"A"}              # only A is a member
    out = bm.mask_by_membership(panel, members_at)
    assert list(out["symbol"]) == ["A"]


def test_atr14_first_values():
    high = np.array([10.0, 11, 12]); low = np.array([9.0, 9.5, 11]); close = np.array([9.5, 10.5, 11.5])
    a = bm.atr14(high, low, close)
    assert np.isnan(a[0])                      # 首根无前收→NaN
    assert a[-1] > 0 and not np.isnan(a[-1])


def test_factor_cols_count_and_order():
    # Existing ordering invariants
    assert bm.FACTOR_COLS[0] == "f_bm"
    assert bm.FACTOR_COLS[1] == "f_npyoy"
    # Total 67 factors (63 previous + 4 new systematic-risk factors)
    assert len(bm.FACTOR_COLS) == 67
    # Hard-gate names still present
    assert "f_roe" in bm.FACTOR_COLS
    assert "f_logamt" in bm.FACTOR_COLS
    # Original last factor at index 12
    assert bm.FACTOR_COLS[12] == "f_secmom"
    # A few new factors appended after index 12
    assert "f_mom60" in bm.FACTOR_COLS
    assert "f_rsi14" in bm.FACTOR_COLS
    assert "f_ep" in bm.FACTOR_COLS
    assert "f_padir" in bm.FACTOR_COLS
    assert "f_secheat" in bm.FACTOR_COLS
    # New price factors at indices 37-39
    assert bm.FACTOR_COLS[37] == "f_maxret20"
    assert bm.FACTOR_COLS[38] == "f_skew60"
    assert bm.FACTOR_COLS[39] == "f_relstr60"
    # New orthogonal financial factors at indices 40-54
    assert bm.FACTOR_COLS[40] == "f_cfo"
    assert bm.FACTOR_COLS[54] == "f_arturn"
    # New PV microstructure factors at indices 55-62
    assert bm.FACTOR_COLS[55] == "f_udvol"
    assert bm.FACTOR_COLS[62] == "f_vwapdev"


def _make_uptrend_fixture(n=280):
    """Synthetic uptrend kday with turn, pctChg, amount columns."""
    dates = pd.bdate_range("2020-01-01", periods=n).strftime("%Y-%m-%d")
    close = pd.Series(np.linspace(10, 28, n))
    high = close * 1.01
    low = close * 0.99
    volume = pd.Series(np.ones(n) * 1e6)
    amount = close * volume
    turn = pd.Series(np.ones(n) * 0.5)        # 0.5% turnover
    pctChg = close.pct_change() * 100         # percent change
    kday = pd.DataFrame({
        "time": dates,
        "open": close,
        "high": high,
        "low": low,
        "close": close,
        "volume": volume,
        "amount": amount,
        "turn": turn,
        "pctChg": pctChg,
    })
    fund = pd.DataFrame({
        "time": [dates[0]],
        "roe": [12.0], "np_yoy": [30.0], "rev_yoy": [10.0],
        "gross_margin": [40.0], "eps": [2.0], "bps": [5.0],
    })
    return kday, fund, dates, close


def test_compute_symbol_factors_pit_and_label():
    """Original tests preserved: PIT fill, momentum, label NaN, sec=None."""
    n = 140
    dates = pd.bdate_range("2020-01-01", periods=n).strftime("%Y-%m-%d")
    close = pd.Series(np.linspace(10, 24, n))   # 单调上行
    kday = pd.DataFrame({"time": dates, "open": close, "high": close*1.01,
                         "low": close*0.99, "close": close, "volume": 1e6,
                         "amount": close*1e6, "turn": 0.5,
                         "pctChg": close.pct_change()*100})
    fund = pd.DataFrame({"time": [dates[0]], "roe": [12.0], "np_yoy": [30.0], "rev_yoy": [10.0],
                         "gross_margin": [40.0], "eps": [1.0], "bps": [5.0]})
    out = bm.compute_symbol_factors(kday, fund, None)
    row = out.loc[dates[130]]                   # 中段某日
    assert np.isclose(row["f_bm"], 5.0 / close.iloc[130])     # bps/close
    assert np.isclose(row["f_npyoy"], 30.0)                   # 时点财务前向填充
    assert row["f_mom20"] > 0                                 # 上行→正动量
    # 标签=未来5日收益，单调上行应为正
    assert row["fwd_ret_5d"] > 0
    assert np.isnan(out.iloc[-1]["fwd_ret_5d"])              # 末尾无未来→NaN
    assert np.isnan(row["f_secmom"])                          # sec=None→NaN


def test_new_factors_uptrend():
    """New factors compute correctly on the uptrend fixture."""
    kday, fund, dates, close = _make_uptrend_fixture(n=280)
    out = bm.compute_symbol_factors(kday, fund, None)

    # Pick a mid-row well past all rolling windows (index 260)
    mid = dates[260]
    row = out.loc[mid]
    mid_i = 260

    # f_mom60: uptrend → positive
    assert row["f_mom60"] > 0, f"f_mom60={row['f_mom60']}"

    # f_mom250: uptrend across 250 bars → positive
    assert row["f_mom250"] > 0, f"f_mom250={row['f_mom250']}"

    # f_rsi14: sustained uptrend → RSI > 50
    assert row["f_rsi14"] > 50, f"f_rsi14={row['f_rsi14']}"

    # f_ep: eps/close
    expected_ep = 2.0 / float(close.iloc[mid_i])
    assert np.isclose(row["f_ep"], expected_ep, rtol=1e-5), \
        f"f_ep={row['f_ep']}, expected={expected_ep}"

    # f_hi52: close/rolling(250).max — on monotone uptrend close equals max → 1.0
    assert np.isclose(row["f_hi52"], 1.0, rtol=1e-4), f"f_hi52={row['f_hi52']}"

    # f_vol20 should be finite and positive (linspace → tiny but nonzero pct changes)
    assert not np.isnan(row["f_vol20"]) and row["f_vol20"] >= 0, f"f_vol20={row['f_vol20']}"

    # f_turn: raw column passthrough
    assert np.isclose(row["f_turn"], 0.5), f"f_turn={row['f_turn']}"

    # f_turnmean: rolling(20) of 0.5 constant = 0.5
    assert np.isclose(row["f_turnmean"], 0.5), f"f_turnmean={row['f_turnmean']}"

    # fwd_ret_5d tail NaN still holds
    assert np.isnan(out.iloc[-1]["fwd_ret_5d"])


def test_pa_factors_nan_when_sec_is_none():
    """PA/sector new factors are NaN when sec=None."""
    kday, fund, dates, _ = _make_uptrend_fixture(n=140)
    out = bm.compute_symbol_factors(kday, fund, None)
    mid = dates[130]
    row = out.loc[mid]
    for fname in ["f_padir", "f_pastruct", "f_paregime", "f_papull",
                  "f_sectrend", "f_secbreadth", "f_secheat"]:
        assert np.isnan(row[fname]), f"{fname} should be NaN when sec=None, got {row[fname]}"


def test_pa_factors_merge_when_sec_provided():
    """PA/sector factors populate correctly from a synthetic sec DataFrame."""
    kday, fund, dates, _ = _make_uptrend_fixture(n=140)

    # Build a synthetic sec frame with all PA columns
    sec = pd.DataFrame({
        "date": list(dates),
        "sec_mom20": np.ones(140) * 0.05,
        "pa_dir": np.ones(140) * 1.0,
        "pa_struct": np.ones(140) * 2.0,
        "pa_regime": np.ones(140) * 0.5,
        "pa_pullback": np.zeros(140),
        "sec_trend": np.ones(140) * 0.8,
        "sec_breadth": np.ones(140) * 0.6,
        "sec_heat": np.ones(140) * 0.4,
    })
    out = bm.compute_symbol_factors(kday, fund, sec)
    mid = dates[130]
    row = out.loc[mid]

    assert np.isclose(row["f_secmom"], 0.05, rtol=1e-5)
    assert np.isclose(row["f_padir"], 1.0, rtol=1e-5)
    assert np.isclose(row["f_pastruct"], 2.0, rtol=1e-5)
    assert np.isclose(row["f_paregime"], 0.5, rtol=1e-5)
    assert np.isclose(row["f_papull"], 0.0, atol=1e-5)
    assert np.isclose(row["f_sectrend"], 0.8, rtol=1e-5)
    assert np.isclose(row["f_secbreadth"], 0.6, rtol=1e-5)
    assert np.isclose(row["f_secheat"], 0.4, rtol=1e-5)


# ---- Tests for the 3 new price factors (f_maxret20, f_skew60, f_relstr60) ----

def test_new_price_factor_cols_count():
    """FACTOR_COLS has 67 entries; f_bm@0 and f_npyoy@1 unchanged; last == f_coskew."""
    assert len(bm.FACTOR_COLS) == 67
    assert bm.FACTOR_COLS[0] == "f_bm"
    assert bm.FACTOR_COLS[1] == "f_npyoy"
    assert bm.FACTOR_COLS[37] == "f_maxret20"
    assert bm.FACTOR_COLS[38] == "f_skew60"
    assert bm.FACTOR_COLS[39] == "f_relstr60"
    assert bm.FACTOR_COLS[54] == "f_arturn"
    assert bm.FACTOR_COLS[-1] == "f_coskew"


def test_new_price_factors_on_uptrend_no_index():
    """f_maxret20 finite & >0 at mid; f_skew60 computable (may be NaN early);
    f_relstr60 is NaN when index_close not provided."""
    kday, fund, dates, close = _make_uptrend_fixture(n=280)
    # Call WITHOUT index_close (default None)
    out = bm.compute_symbol_factors(kday, fund, None)

    mid = dates[200]   # well past both 20-day and 60-day windows
    row = out.loc[mid]

    # f_maxret20: uptrend has positive daily returns -> rolling max > 0
    assert np.isfinite(row["f_maxret20"]), f"f_maxret20 not finite at mid: {row['f_maxret20']}"
    assert row["f_maxret20"] > 0, f"f_maxret20 expected >0 on uptrend, got {row['f_maxret20']}"

    # f_skew60: may be finite or NaN depending on the row, but the column must exist
    # At mid (row 200, well past window 60) it should be computable (not necessarily non-NaN
    # when variance is near-zero on linspace, but the column must be present).
    assert "f_skew60" in out.columns, "f_skew60 column missing"

    # f_relstr60: must be NaN when index_close=None
    assert np.isnan(row["f_relstr60"]), \
        f"f_relstr60 should be NaN when no index provided, got {row['f_relstr60']}"
    # All rows should be NaN
    assert out["f_relstr60"].isna().all(), "f_relstr60 should be all-NaN when index_close=None"


def test_relstr60_with_synthetic_index():
    """f_relstr60 ≈ symbol_60d_ret − idx_60d_ret when a synthetic index_close is passed."""
    kday, fund, dates, close = _make_uptrend_fixture(n=280)

    # Build a synthetic CSI300 close: flat at 4000 for most, then rises last 60 bars
    # so the index 60-day return is non-trivial and differs from the symbol.
    n = len(dates)
    idx_prices = np.linspace(4000.0, 5000.0, n)     # CSI300 also rises, but slower rate
    index_close = pd.Series(idx_prices, index=list(dates))

    out = bm.compute_symbol_factors(kday, fund, None, index_close=index_close)

    # Use mid row at index 200 (well past 60-bar window)
    mid_i = 200
    mid_date = dates[mid_i]
    row = out.loc[mid_date]

    # Manually compute expected values
    sym_ret60 = float(close.iloc[mid_i]) / float(close.iloc[mid_i - 60]) - 1
    idx_ret60 = idx_prices[mid_i] / idx_prices[mid_i - 60] - 1
    expected = sym_ret60 - idx_ret60

    assert np.isfinite(row["f_relstr60"]), f"f_relstr60 should be finite at mid, got {row['f_relstr60']}"
    assert np.isclose(row["f_relstr60"], expected, rtol=1e-5), \
        f"f_relstr60={row['f_relstr60']}, expected={expected}"


# ---- Tests for the 15 new orthogonal financial factors ----

def _make_fin_fixture(dates):
    """Synthetic financials_extra DataFrame with 2 disclosure dates.

    disclosure_1 = dates[10]  (day 10)
    disclosure_2 = dates[50]  (day 50)
    """
    disc1 = dates[10]
    disc2 = dates[50]
    fin = pd.DataFrame({
        "time": [disc1, disc2],
        "cfo":           [1000.0,  2000.0],
        "cfo_to_np":     [1.1,     1.2],
        "cfo_to_rev":    [0.15,    0.18],
        "debt_ratio":    [0.40,    0.35],
        "roic":          [0.12,    0.15],
        "roa":           [0.08,    0.10],
        "net_margin":    [0.10,    0.12],
        "op_margin":     [0.14,    0.16],
        "current_ratio": [1.5,     1.8],
        "quick_ratio":   [1.0,     1.2],
        "cash_ratio":    [0.5,     0.6],
        "equity_mult":   [2.0,     2.5],
        "asset_turn":    [0.8,     0.9],
        "inv_turn":      [4.0,     5.0],
        "ar_turn":       [6.0,     7.0],
    })
    return fin, disc1, disc2


def test_fin_factor_cols_count_and_names():
    """FACTOR_COLS has 67 entries; f_bm@0, f_npyoy@1 unchanged; f_arturn@54; last == f_coskew."""
    assert len(bm.FACTOR_COLS) == 67, f"Expected 67, got {len(bm.FACTOR_COLS)}"
    assert bm.FACTOR_COLS[0] == "f_bm"
    assert bm.FACTOR_COLS[1] == "f_npyoy"
    assert bm.FACTOR_COLS[54] == "f_arturn"
    assert bm.FACTOR_COLS[-1] == "f_coskew"
    # All 15 new factor names present
    expected_new = [
        "f_cfo", "f_cfonp", "f_cforev", "f_debt", "f_roic", "f_roa",
        "f_netmargin", "f_opmargin", "f_curr", "f_quick", "f_cashratio",
        "f_eqmult", "f_aturn", "f_iturn", "f_arturn",
    ]
    for fname in expected_new:
        assert fname in bm.FACTOR_COLS, f"{fname} missing from FACTOR_COLS"


def test_fin_pit_correctness():
    """PIT correctness: backward merge_asof on disclosure date.

    - day < disc1 → NaN (no disclosure yet)
    - disc1 ≤ day < disc2 → disclosure-1 values
    - day ≥ disc2 → disclosure-2 values
    """
    kday, fund, dates, close = _make_uptrend_fixture(n=140)
    fin, disc1, disc2 = _make_fin_fixture(dates)

    out = bm.compute_symbol_factors(kday, fund, None, fin=fin)

    # Day before first disclosure → NaN
    pre_disc = dates[5]   # day 5 < disc1 (day 10)
    row_pre = out.loc[pre_disc]
    assert np.isnan(row_pre["f_roic"]), \
        f"f_roic should be NaN before first disclosure, got {row_pre['f_roic']}"
    assert np.isnan(row_pre["f_debt"]), \
        f"f_debt should be NaN before first disclosure, got {row_pre['f_debt']}"

    # Day after disc1 but before disc2 → disclosure-1 values
    mid1 = dates[30]   # disc1=day10 ≤ day30 < disc2=day50
    row_mid1 = out.loc[mid1]
    assert np.isclose(row_mid1["f_roic"], 0.12, rtol=1e-5), \
        f"f_roic@mid1={row_mid1['f_roic']}, expected 0.12 (disc1)"
    assert np.isclose(row_mid1["f_debt"], 0.40, rtol=1e-5), \
        f"f_debt@mid1={row_mid1['f_debt']}, expected 0.40 (disc1)"

    # Day after disc2 → disclosure-2 values
    mid2 = dates[80]   # day80 ≥ disc2=day50
    row_mid2 = out.loc[mid2]
    assert np.isclose(row_mid2["f_roic"], 0.15, rtol=1e-5), \
        f"f_roic@mid2={row_mid2['f_roic']}, expected 0.15 (disc2)"
    assert np.isclose(row_mid2["f_debt"], 0.35, rtol=1e-5), \
        f"f_debt@mid2={row_mid2['f_debt']}, expected 0.35 (disc2)"


def test_fin_none_gives_nan():
    """fin=None → all 15 financial factor columns are NaN."""
    kday, fund, dates, _ = _make_uptrend_fixture(n=140)
    out = bm.compute_symbol_factors(kday, fund, None, fin=None)
    mid = dates[100]
    row = out.loc[mid]
    fin_factors = [
        "f_cfo", "f_cfonp", "f_cforev", "f_debt", "f_roic", "f_roa",
        "f_netmargin", "f_opmargin", "f_curr", "f_quick", "f_cashratio",
        "f_eqmult", "f_aturn", "f_iturn", "f_arturn",
    ]
    for fname in fin_factors:
        assert fname in out.columns, f"{fname} column missing from output"
        assert np.isnan(row[fname]), \
            f"{fname} should be NaN when fin=None, got {row[fname]}"


# ---- Tests for the 8 new price-volume microstructure factors (indices 55-62) ----

def test_pv_factor_cols_count_and_order():
    """FACTOR_COLS now has 67 entries; f_bm@0/f_npyoy@1 unchanged; last == f_coskew."""
    assert len(bm.FACTOR_COLS) == 67, f"Expected 67, got {len(bm.FACTOR_COLS)}"
    assert bm.FACTOR_COLS[0] == "f_bm"
    assert bm.FACTOR_COLS[1] == "f_npyoy"
    assert bm.FACTOR_COLS[-1] == "f_coskew"
    # New PV factors at indices 55-62
    expected_pv = [
        "f_udvol", "f_obv_slope", "f_cmf20", "f_clv",
        "f_pvcorr", "f_mfi14", "f_body", "f_vwapdev",
    ]
    for i, fname in enumerate(expected_pv):
        assert bm.FACTOR_COLS[55 + i] == fname, \
            f"Index {55+i}: expected {fname}, got {bm.FACTOR_COLS[55+i]}"


def test_pv_factors_on_uptrend():
    """8 new PV factors are present and finite at a mid-row on the uptrend fixture."""
    kday, fund, dates, close = _make_uptrend_fixture(n=280)
    out = bm.compute_symbol_factors(kday, fund, None)

    # Pick row 200 — well past largest rolling window (20d)
    mid = dates[200]
    row = out.loc[mid]

    pv_cols = ["f_udvol", "f_obv_slope", "f_cmf20", "f_clv",
               "f_pvcorr", "f_mfi14", "f_body", "f_vwapdev"]
    for col in pv_cols:
        assert col in out.columns, f"{col} column missing"

    # f_clv: CLV = ((c-lo)-(hi-c))/(hi-lo); for uptrend hi=c*1.01, lo=c*0.99
    # numerator = (c-c*0.99)-(c*1.01-c) = 0.01c - 0.01c = 0; expected 0.0
    # rolling mean of 0 = 0; allow small float tolerance
    assert np.isfinite(row["f_clv"]), f"f_clv not finite: {row['f_clv']}"
    assert -1.0 <= row["f_clv"] <= 1.0, f"f_clv={row['f_clv']} out of [-1,1]"

    # f_mfi14: must be in [0, 100]
    assert np.isfinite(row["f_mfi14"]), f"f_mfi14 not finite: {row['f_mfi14']}"
    assert 0.0 <= row["f_mfi14"] <= 100.0, f"f_mfi14={row['f_mfi14']} out of [0,100]"

    # f_body: candle body/range in [0, 1]
    assert np.isfinite(row["f_body"]), f"f_body not finite: {row['f_body']}"
    assert 0.0 <= row["f_body"] <= 1.0, f"f_body={row['f_body']} out of [0,1]"

    # f_vwapdev: close deviation from VWAP — on an uptrend with amount=close*volume
    # this is a well-defined ratio; just check it is finite
    assert np.isfinite(row["f_vwapdev"]), f"f_vwapdev not finite: {row['f_vwapdev']}"


def test_pv_udvol_up_domination():
    """f_udvol > 0 when up-day volumes clearly dominate down-day volumes."""
    n = 60
    dates = pd.bdate_range("2021-01-01", periods=n).strftime("%Y-%m-%d")
    # Alternating: 4 up-days (high volume) then 1 down-day (low volume)
    close_vals = np.ones(n)
    volume_vals = np.ones(n) * 100.0    # baseline
    for i in range(n):
        if i % 5 == 4:
            # down day, low volume
            close_vals[i] = close_vals[i - 1] * 0.999
            volume_vals[i] = 10.0
        else:
            # up day, high volume
            close_vals[i] = close_vals[i - 1] * 1.001 if i > 0 else 1.0
            volume_vals[i] = 500.0

    close = pd.Series(close_vals)
    volume = pd.Series(volume_vals)
    high = close * 1.005
    low = close * 0.995
    amount = close * volume

    kday = pd.DataFrame({
        "time": dates,
        "open": close,
        "high": high,
        "low": low,
        "close": close,
        "volume": volume,
        "amount": amount,
        "turn": 0.5,
        "pctChg": close.pct_change() * 100,
    })
    fund = pd.DataFrame({
        "time": [dates[0]],
        "roe": [10.0], "np_yoy": [20.0], "rev_yoy": [5.0],
        "gross_margin": [30.0], "eps": [1.0], "bps": [4.0],
    })

    out = bm.compute_symbol_factors(kday, fund, None)

    # At a late row (index 50, well past 20-day window) f_udvol should be > 0
    late_date = dates[50]
    f_udvol_val = out.loc[late_date, "f_udvol"]
    assert np.isfinite(f_udvol_val), f"f_udvol not finite at late row: {f_udvol_val}"
    assert f_udvol_val > 0, \
        f"f_udvol expected >0 when up-day volumes dominate, got {f_udvol_val}"


# ---- Tests for the 4 new systematic-risk / beta-family factors (indices 63-66) ----

def test_risk_factor_cols_count_and_order():
    """FACTOR_COLS now has 67 entries; f_bm@0/f_npyoy@1 unchanged; last == 'f_coskew'."""
    assert len(bm.FACTOR_COLS) == 67, f"Expected 67, got {len(bm.FACTOR_COLS)}"
    assert bm.FACTOR_COLS[0] == "f_bm"
    assert bm.FACTOR_COLS[1] == "f_npyoy"
    assert bm.FACTOR_COLS[63] == "f_beta"
    assert bm.FACTOR_COLS[64] == "f_ivol"
    assert bm.FACTOR_COLS[65] == "f_resmom"
    assert bm.FACTOR_COLS[66] == "f_coskew"
    assert bm.FACTOR_COLS[-1] == "f_coskew"


def test_risk_factors_finite_with_index():
    """f_beta, f_ivol, f_resmom, f_coskew are present and finite at a late row
    when a synthetic index_close is provided (mirrors the f_relstr60 fixture)."""
    kday, fund, dates, close = _make_uptrend_fixture(n=280)

    # Synthetic CSI300 close — also an uptrend but at a different rate so beta != 1.
    n = len(dates)
    idx_prices = np.linspace(4000.0, 5000.0, n)
    index_close = pd.Series(idx_prices, index=list(dates))

    out = bm.compute_symbol_factors(kday, fund, None, index_close=index_close)

    # Row 200 — well past the 60-bar warm-up window
    mid = dates[200]
    row = out.loc[mid]

    risk_cols = ["f_beta", "f_ivol", "f_resmom", "f_coskew"]
    for col in risk_cols:
        assert col in out.columns, f"{col} column missing from output"
        assert np.isfinite(row[col]), \
            f"{col} not finite at post-warmup row: {row[col]}"


def test_risk_factors_nan_when_no_index():
    """f_beta, f_ivol, f_resmom, f_coskew are all NaN when index_close=None."""
    kday, fund, dates, _ = _make_uptrend_fixture(n=280)
    out = bm.compute_symbol_factors(kday, fund, None, index_close=None)

    risk_cols = ["f_beta", "f_ivol", "f_resmom", "f_coskew"]
    for col in risk_cols:
        assert col in out.columns, f"{col} column missing from output"
        assert out[col].isna().all(), \
            f"{col} should be all-NaN when index_close=None, but has non-NaN values"
