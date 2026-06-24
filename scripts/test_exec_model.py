"""TDD for exec_model.py — A-share execution-realism primitives.

Pure functions only (no data I/O):
  - board_limit_pct: per-board, date-aware daily price-limit (percent)
  - is_locked_up / is_locked_down: limit-lock detection (can't buy / can't sell)
  - sqrt_impact_bps: participation-dependent square-root market-impact cost
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import exec_model as em


# ---------------------------------------------------------------------------
# board_limit_pct(symbol, date) -> float (percent)
# ---------------------------------------------------------------------------

def test_sh_main_board_is_10pct():
    assert em.board_limit_pct("sh600000", "2021-05-10") == 10.0


def test_sz_main_board_is_10pct():
    assert em.board_limit_pct("sz000001", "2021-05-10") == 10.0


def test_sz_sme_board_is_10pct():
    # 中小板 002xxx is main-board 10%
    assert em.board_limit_pct("sz002731", "2021-05-10") == 10.0


def test_star_market_is_20pct():
    # 科创板 sh688xxx — 20% since launch (2019)
    assert em.board_limit_pct("sh688689", "2021-05-10") == 20.0


def test_chinext_is_20pct_after_reform():
    # 创业板 sz300xxx — 20% on/after 2020-08-24
    assert em.board_limit_pct("sz300750", "2021-05-10") == 20.0


def test_chinext_is_10pct_before_reform():
    assert em.board_limit_pct("sz300750", "2019-05-10") == 10.0


def test_chinext_reform_date_inclusive():
    assert em.board_limit_pct("sz300750", "2020-08-24") == 20.0
    assert em.board_limit_pct("sz300750", "2020-08-23") == 10.0


def test_chinext_new_301_always_20pct():
    # sz301xxx launched after the 2020 reform → always 20%
    assert em.board_limit_pct("sz301316", "2023-05-10") == 20.0


# ---------------------------------------------------------------------------
# is_locked_up(pct_chg, close, high, limit_pct, tol) -> bool
#   "can't buy": closed locked at the upper limit
# ---------------------------------------------------------------------------

def test_locked_up_true_at_limit():
    # +9.98% and close == high → locked limit-up, can't buy
    assert em.is_locked_up(9.98, 11.0, 11.0, 10.0) is True


def test_locked_up_false_when_far_from_limit():
    assert em.is_locked_up(5.0, 10.5, 11.0, 10.0) is False


def test_locked_up_false_when_touched_but_closed_below_high():
    # hit +10% intraday (high) but closed at +8% (close < high) → tradable
    assert em.is_locked_up(8.0, 10.8, 11.0, 10.0) is False


def test_locked_up_true_for_star_20pct():
    assert em.is_locked_up(19.9, 12.0, 12.0, 20.0) is True


def test_locked_up_tolerance_boundary():
    # default tol = 0.3pp: 9.70 counts as locked, 9.69 does not (close == high)
    assert em.is_locked_up(9.70, 11.0, 11.0, 10.0) is True
    assert em.is_locked_up(9.69, 11.0, 11.0, 10.0) is False


def test_locked_up_handles_nan_safely():
    # missing pctChg → treat as not locked (conservative: don't drop a tradable name on missing data)
    assert em.is_locked_up(float("nan"), 11.0, 11.0, 10.0) is False


# ---------------------------------------------------------------------------
# is_locked_down(pct_chg, close, low, limit_pct, tol) -> bool
#   "can't sell": closed locked at the lower limit
# ---------------------------------------------------------------------------

def test_locked_down_true_at_limit():
    assert em.is_locked_down(-9.98, 9.0, 9.0, 10.0) is True


def test_locked_down_false_when_close_above_low():
    # down a lot but closed above the low → there was a bid, can sell
    assert em.is_locked_down(-9.98, 9.1, 9.0, 10.0) is False


def test_locked_down_false_when_far_from_limit():
    assert em.is_locked_down(-3.0, 9.5, 9.0, 10.0) is False


# ---------------------------------------------------------------------------
# sqrt_impact_bps(notional, adv, k) -> float (bps)
#   participation = notional / adv ; impact = k * sqrt(participation)
# ---------------------------------------------------------------------------

def test_impact_zero_when_no_trade():
    assert em.sqrt_impact_bps(0.0, 1e8, 100.0) == 0.0


def test_impact_equals_k_at_full_participation():
    # notional == adv → participation 1 → impact == k bps
    assert em.sqrt_impact_bps(1e8, 1e8, 100.0) == 100.0


def test_impact_sqrt_scaling_at_1pct():
    # participation 0.01 → sqrt = 0.1 → 10 bps for k=100
    assert abs(em.sqrt_impact_bps(1e6, 1e8, 100.0) - 10.0) < 1e-9


def test_impact_quadruple_notional_doubles_cost():
    a = em.sqrt_impact_bps(1e6, 1e8, 100.0)
    b = em.sqrt_impact_bps(4e6, 1e8, 100.0)
    assert abs(b - 2 * a) < 1e-9


def test_impact_infinite_when_no_liquidity():
    # adv <= 0 → effectively untradable
    assert em.sqrt_impact_bps(1e6, 0.0, 100.0) == float("inf")
