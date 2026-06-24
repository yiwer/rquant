"""A-share execution-realism primitives (pure functions, no I/O).

Used by the ridge-on-gauss execution stress test:
  - board_limit_pct: per-board, date-aware daily price-limit (percent)
  - is_locked_up / is_locked_down: limit-lock detection (can't buy / can't sell)
  - sqrt_impact_bps: participation-dependent square-root market-impact cost
"""
import math

# 创业板 (ChiNext) widened from ±10% to ±20% on this date.
CHINEXT_REFORM_DATE = "2020-08-24"


def _is_nan(x):
    return x is None or x != x


def board_limit_pct(symbol, date):
    """Daily price-limit (in percent) for an A-share symbol on a given date.

    sh688xxx 科创板 STAR  → 20%  (since 2019 launch)
    sz300xxx 创业板 ChiNext → 20% on/after 2020-08-24, else 10%
    sz301/302xxx newer ChiNext → 20% (listed post-reform)
    everything else (sh60xxxx / sz00xxxx / 中小板) → 10%
    ST names are excluded upstream (would be ±5%).
    """
    s = symbol.lower()
    if s.startswith("sh688"):
        return 20.0
    if s.startswith("sz300"):
        return 20.0 if date >= CHINEXT_REFORM_DATE else 10.0
    if s.startswith("sz301") or s.startswith("sz302"):
        return 20.0
    return 10.0


def is_locked_up(pct_chg, close, high, limit_pct, tol=0.3):
    """True if the bar closed locked at the upper limit → you can't buy here.

    Requires both (a) the day's % change is within `tol` of the limit and
    (b) the close is at the high (no supply above → bid-locked). A name that
    merely *touched* the limit intraday but closed below the high was tradable.
    Missing pct_chg → not locked (don't drop a name on missing data).
    """
    if _is_nan(pct_chg):
        return False
    return (pct_chg >= limit_pct - tol) and (close >= high - 1e-9)


def is_locked_down(pct_chg, close, low, limit_pct, tol=0.3):
    """True if the bar closed locked at the lower limit → you can't sell here."""
    if _is_nan(pct_chg):
        return False
    return (pct_chg <= -(limit_pct - tol)) and (close <= low + 1e-9)


def sqrt_impact_bps(notional, adv, k):
    """Square-root market-impact cost in bps.

    participation = notional / adv ; impact = k * sqrt(participation).
    k is the impact at 100% participation (bps). notional≤0 → 0; adv≤0 → inf.
    """
    if notional <= 0:
        return 0.0
    if adv <= 0:
        return float("inf")
    return k * math.sqrt(notional / adv)
