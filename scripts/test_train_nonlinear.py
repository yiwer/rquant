# scripts/test_train_nonlinear.py
"""Unit tests for train_nonlinear.py — synthetic panel only, no data/ needed."""
import numpy as np
import pandas as pd
import pytest
import train_nonlinear as tn
from build_factor_matrix import FACTOR_COLS


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _make_panel(date_lo, date_hi, n_stocks=40, seed=0, signal_col=0, signal_strength=1.5):
    """Build a synthetic (date, symbol, *factors, fwd_ret_5d) panel.

    signal_col factor is the dominant predictor (coefficient = signal_strength).
    """
    rng = np.random.default_rng(seed)
    rows = []
    for d in pd.bdate_range(date_lo, date_hi, freq="5B").strftime("%Y-%m-%d"):
        for s in range(n_stocks):
            x = rng.normal(size=len(FACTOR_COLS))
            fwd = signal_strength * x[signal_col] + rng.normal(scale=0.3)
            rows.append([d, f"s{s}", *x, fwd])
    return pd.DataFrame(rows, columns=["date", "symbol", *FACTOR_COLS, "fwd_ret_5d"])


def _make_full_panel(seed=0):
    """Panel spanning all WFO windows (2018-2026)."""
    return _make_panel("2018-01-02", "2026-06-30", n_stocks=40, seed=seed)


# ---------------------------------------------------------------------------
# WFO_FOLDS structure
# ---------------------------------------------------------------------------

def test_wfo_folds_count():
    assert len(tn.WFO_FOLDS) == 4


def test_wfo_folds_train_lo_fixed():
    """All folds share the same anchored train start."""
    for fold in tn.WFO_FOLDS:
        assert fold[0] == "2018-01-02", f"train_lo mismatch: {fold}"


def test_wfo_folds_train_hi_sequence():
    """Train ends advance year by year: 2021, 2022, 2023, 2024."""
    expected_his = ["2021-12-31", "2022-12-31", "2023-12-31", "2024-12-31"]
    actual_his = [f[1] for f in tn.WFO_FOLDS]
    assert actual_his == expected_his


def test_wfo_folds_oos_follows_train():
    """OOS window starts the day after train_hi (year boundary)."""
    for fold in tn.WFO_FOLDS:
        train_lo, train_hi, oos_lo, oos_hi = fold
        # OOS start must be > train_hi
        assert oos_lo > train_hi, f"OOS overlaps train: {fold}"
        # OOS start year = train_hi year + 1
        train_year = int(train_hi[:4])
        oos_year = int(oos_lo[:4])
        assert oos_year == train_year + 1, f"OOS year mismatch: {fold}"


def test_wfo_folds_no_train_oos_overlap():
    """OOS and train windows must not overlap."""
    for fold in tn.WFO_FOLDS:
        train_lo, train_hi, oos_lo, oos_hi = fold
        assert oos_lo > train_hi
        assert oos_hi > oos_lo


# ---------------------------------------------------------------------------
# select_interactions
# ---------------------------------------------------------------------------

def test_select_interactions_uses_only_train():
    """CRITICAL no-lookahead test (from task brief).

    Train window: f_bm (index 0) is the dominant signal → should be selected.
    """
    rng = np.random.default_rng(0)
    rows = []
    for d in pd.bdate_range("2018-01-02", "2021-12-31", freq="5B").strftime("%Y-%m-%d"):
        for s in range(40):
            x = rng.normal(size=len(FACTOR_COLS))
            fwd = 1.5 * x[0] + rng.normal(scale=0.3)
            rows.append([d, f"s{s}", *x, fwd])
    p = pd.DataFrame(rows, columns=["date", "symbol", *FACTOR_COLS, "fwd_ret_5d"])
    pairs = tn.select_interactions(p, FACTOR_COLS, "2018-01-02", "2021-12-31", k=5)
    assert any(0 in pair for pair in pairs)   # f_bm (index 0) should be among top-5


def test_select_interactions_returns_ck2_pairs():
    """Returns C(k,2) = k*(k-1)/2 pairs for k=5."""
    panel = _make_panel("2018-01-02", "2021-12-31")
    pairs = tn.select_interactions(panel, FACTOR_COLS, "2018-01-02", "2021-12-31", k=5)
    assert len(pairs) == 10   # C(5,2) = 10
    for p in pairs:
        assert len(p) == 2
        assert p[0] != p[1]


def test_select_interactions_k1_returns_empty():
    """k=1 → C(1,2)=0 pairs."""
    panel = _make_panel("2018-01-02", "2021-12-31")
    pairs = tn.select_interactions(panel, FACTOR_COLS, "2018-01-02", "2021-12-31", k=1)
    assert len(pairs) == 0


def test_select_interactions_does_not_read_oos():
    """OOS rows carry a reversed signal — if select used OOS it would pick a different factor.

    We verify the selection is identical with and without OOS rows present in the panel.
    """
    # Panel with only train data
    panel_train_only = _make_panel("2018-01-02", "2021-12-31", signal_col=2)
    pairs_no_oos = tn.select_interactions(
        panel_train_only, FACTOR_COLS, "2018-01-02", "2021-12-31", k=5
    )

    # Same train data + OOS with reversed signal on a different factor
    rng2 = np.random.default_rng(99)
    oos_rows = []
    for d in pd.bdate_range("2022-01-02", "2022-12-31", freq="5B").strftime("%Y-%m-%d"):
        for s in range(40):
            x = rng2.normal(size=len(FACTOR_COLS))
            # Completely different dominant factor (index 11) in OOS
            fwd = 5.0 * x[11] + rng2.normal(scale=0.01)
            oos_rows.append([d, f"s{s}", *x, fwd])
    oos_df = pd.DataFrame(oos_rows, columns=["date", "symbol", *FACTOR_COLS, "fwd_ret_5d"])
    panel_with_oos = pd.concat([panel_train_only, oos_df], ignore_index=True)

    pairs_with_oos = tn.select_interactions(
        panel_with_oos, FACTOR_COLS, "2018-01-02", "2021-12-31", k=5
    )
    # Selections must be identical regardless of OOS being in panel
    assert set(map(frozenset, pairs_no_oos)) == set(map(frozenset, pairs_with_oos))


# ---------------------------------------------------------------------------
# train_fold
# ---------------------------------------------------------------------------

def test_train_fold_returns_required_keys():
    """train_fold must return dict with weights, alpha, interaction_pairs, feat_names."""
    panel = _make_panel("2018-01-02", "2023-12-31", n_stocks=30)
    fold = ("2018-01-02", "2021-12-31", "2022-01-02", "2022-12-31")
    result = tn.train_fold(panel, fold)
    for key in ("weights", "alpha", "interaction_pairs", "feat_names"):
        assert key in result, f"Missing key: {key}"


def test_train_fold_weight_length_matches_feat_names():
    panel = _make_panel("2018-01-02", "2023-12-31", n_stocks=30)
    fold = ("2018-01-02", "2021-12-31", "2022-01-02", "2022-12-31")
    result = tn.train_fold(panel, fold)
    assert len(result["weights"]) == len(result["feat_names"])


def test_train_fold_alpha_in_allowed_set():
    """Selected alpha must come from the predefined ALPHAS list."""
    panel = _make_panel("2018-01-02", "2023-12-31", n_stocks=30)
    fold = ("2018-01-02", "2021-12-31", "2022-01-02", "2022-12-31")
    result = tn.train_fold(panel, fold)
    assert result["alpha"] in tn.ALPHAS


def test_train_fold_nonlinear_features_included():
    """feat_names must contain both squared and interaction terms."""
    panel = _make_panel("2018-01-02", "2023-12-31", n_stocks=30)
    fold = ("2018-01-02", "2021-12-31", "2022-01-02", "2022-12-31")
    result = tn.train_fold(panel, fold)
    # Squared features: names contain "^2"
    sq_names = [n for n in result["feat_names"] if "^2" in n]
    assert len(sq_names) > 0, "No squared features found"
    # Interaction features: names contain "x"
    inter_names = [n for n in result["feat_names"] if "x" in n and "^2" not in n
                   and "f_" not in n.split("x")[0].strip()]
    # At least C(k,2)=10 interaction features for k=5 default
    pairs = result["interaction_pairs"]
    assert len(pairs) == 10   # C(5,2) for default k=5


def test_train_fold_inner_split_within_train():
    """Inner validation split must stay within the fold's train window.

    We spy by checking that reducing the panel to train-only gives same result
    as a panel that also includes OOS rows — proving OOS is not read.
    """
    train_lo, train_hi = "2018-01-02", "2021-12-31"
    oos_lo, oos_hi = "2022-01-02", "2022-12-31"
    panel_train = _make_panel(train_lo, train_hi, n_stocks=30, seed=7)
    fold = (train_lo, train_hi, oos_lo, oos_hi)

    # Add OOS rows with completely inverted signal
    rng = np.random.default_rng(42)
    oos_rows = []
    for d in pd.bdate_range(oos_lo, oos_hi, freq="5B").strftime("%Y-%m-%d"):
        for s in range(30):
            x = rng.normal(size=len(FACTOR_COLS))
            fwd = -99.0 * x[0]   # extreme reversed signal
            oos_rows.append([d, f"s{s}", *x, fwd])
    oos_df = pd.DataFrame(oos_rows, columns=["date", "symbol", *FACTOR_COLS, "fwd_ret_5d"])
    panel_full = pd.concat([panel_train, oos_df], ignore_index=True)

    result_train_only = tn.train_fold(panel_train, fold)
    result_full = tn.train_fold(panel_full, fold)

    # Alpha selection must be the same (OOS not read during inner split)
    assert result_train_only["alpha"] == result_full["alpha"]
    # Weights must be numerically identical
    np.testing.assert_allclose(
        result_train_only["weights"],
        result_full["weights"],
        rtol=1e-10,
        err_msg="Weights differ — OOS data may have leaked into training",
    )


# ---------------------------------------------------------------------------
# main / output structure
# ---------------------------------------------------------------------------

def test_main_output_structure(tmp_path, monkeypatch):
    """main() writes weights_nonlinear.json with correct structure."""
    import json, train_nonlinear as tn2

    # Patch OUT_DIR and WFO_FOLDS to speed up test (1 tiny fold)
    tiny_panel = _make_panel("2018-01-02", "2023-12-31", n_stocks=20, seed=1)
    tiny_folds = [("2018-01-02", "2021-12-31", "2022-01-02", "2022-12-31")]

    out_file = str(tmp_path / "weights_nonlinear.json")
    monkeypatch.setattr(tn2, "WFO_FOLDS", tiny_folds)
    monkeypatch.setattr(tn2, "WEIGHTS_NL", out_file)

    # Patch panel loading
    monkeypatch.setattr(tn2, "_load_panel", lambda: tiny_panel)

    tn2.main()

    with open(out_file, encoding="utf-8") as f:
        data = json.load(f)

    assert "folds" in data
    assert len(data["folds"]) == 1

    fold_rec = data["folds"][0]
    for key in ("train_lo", "train_hi", "oos_lo", "oos_hi",
                "weights", "alpha", "interaction_pairs", "feat_names"):
        assert key in fold_rec, f"Missing key in fold record: {key}"

    # weights is a list of floats
    assert isinstance(fold_rec["weights"], list)
    assert all(isinstance(w, float) for w in fold_rec["weights"])
