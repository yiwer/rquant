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
    # Total 37 factors (13 original + 24 new)
    assert len(bm.FACTOR_COLS) == 37
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
