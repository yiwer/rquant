"""多折 WFO 非线性因子权重训练：expand_features + 成对交互 + 每折 α 内层切。

锚定扩展：train 固定从 2018-01-02 起，train_hi 逐年推进；OOS = 次年全年。
OOS 窗口仅用于定义折边界，绝不读入任何拟合/选择环节。

产出：data/factor_panel/weights_nonlinear.json
"""
import sys
sys.stdout.reconfigure(encoding="utf-8")
import os
import json
import itertools
import numpy as np
import pandas as pd

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import factor_lib as fl
from build_factor_matrix import FACTOR_COLS, OUT as PANEL, OUT_DIR

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

# Anchored-expanding WFO folds: (train_lo, train_hi, oos_lo, oos_hi)
# train expands year by year; OOS = full following year (incl. 2026 H1 for last fold)
WFO_FOLDS = [
    ("2018-01-02", "2021-12-31", "2022-01-02", "2022-12-31"),
    ("2018-01-02", "2022-12-31", "2023-01-02", "2023-12-31"),
    ("2018-01-02", "2023-12-31", "2024-01-02", "2024-12-31"),
    ("2018-01-02", "2024-12-31", "2025-01-02", "2026-06-30"),
]

ALPHAS = [0.001, 0.003, 0.01, 0.03, 0.1]

WEIGHTS_NL = os.path.join(OUT_DIR, "weights_nonlinear.json")


# ---------------------------------------------------------------------------
# Feature construction helpers
# ---------------------------------------------------------------------------

def _build_xy_expanded(panel, date_lo, date_hi, interaction_pairs):
    """Window slice → (Xexp, y, dates).

    Per-date cross-sectional rank → expand_features(rank, interaction_pairs) → stack.
    Rows with fwd_ret_5d = NaN are dropped. Dates with <5 valid rows are skipped.
    """
    sub = panel[(panel["date"] >= date_lo) & (panel["date"] <= date_hi)]
    Xs, ys, ds = [], [], []
    for d, g in sub.groupby("date"):
        g = g.dropna(subset=["fwd_ret_5d"])
        if len(g) < 5:
            continue
        Xr = fl.rank_columns(g[FACTOR_COLS].to_numpy(float))
        Xexp = fl.expand_features(Xr, interaction_pairs)
        yr = fl.cross_sectional_rank(g["fwd_ret_5d"].to_numpy(float))
        Xs.append(Xexp)
        ys.append(yr)
        ds += [d] * len(g)
    if not Xs:
        raise ValueError(f"No valid rows in [{date_lo}, {date_hi}]")
    return np.vstack(Xs), np.concatenate(ys), ds


def _build_feat_names(interaction_pairs):
    """Build human-readable feature names for expanded feature matrix.

    Order: [original | squared | interactions] — mirrors expand_features layout.
    """
    p = len(FACTOR_COLS)
    orig = list(FACTOR_COLS)
    squared = [f"{c}^2" for c in FACTOR_COLS]
    inter = [f"{FACTOR_COLS[i]}x{FACTOR_COLS[j]}" for i, j in interaction_pairs]
    return orig + squared + inter


# ---------------------------------------------------------------------------
# Interaction selection (train-only, no lookahead)
# ---------------------------------------------------------------------------

def select_interactions(panel, cols, train_lo, train_hi, k=5):
    """Select top-k factors by |train Rank-IC| and return their C(k,2) pairs.

    Only the rows within [train_lo, train_hi] are used — OOS rows in panel
    are ignored even if present.

    Returns:
        list[tuple[int, int]]: column-index pairs (i, j) with i < j.
    """
    sub = panel[(panel["date"] >= train_lo) & (panel["date"] <= train_hi)]
    n_cols = len(cols)
    ic_acc = [[] for _ in range(n_cols)]

    for _, g in sub.groupby("date"):
        g = g.dropna(subset=["fwd_ret_5d"])
        if len(g) < 5:
            continue
        fwd = g["fwd_ret_5d"].to_numpy(float)
        for ci, col in enumerate(cols):
            ic = fl.rank_ic(g[col].to_numpy(float), fwd)
            if not np.isnan(ic):
                ic_acc[ci].append(ic)

    mean_abs_ic = np.array([
        float(np.mean(np.abs(v))) if v else 0.0
        for v in ic_acc
    ])

    # Top-k factor indices by |Rank-IC|
    top_k_idx = list(np.argsort(mean_abs_ic)[::-1][:k])

    # All C(k,2) pairs of column indices (i < j)
    pairs = list(itertools.combinations(top_k_idx, 2))
    return pairs


# ---------------------------------------------------------------------------
# Per-fold alpha selection (inner split entirely within train)
# ---------------------------------------------------------------------------

def _select_alpha_for_fold(panel, train_lo, train_hi, interaction_pairs):
    """Pick best alpha via inner split (last year of train = validation).

    The inner fit window is [train_lo, inner_fit_hi].
    The inner val window is [inner_val_lo, train_hi].
    Both halves are strictly inside [train_lo, train_hi].
    """
    train_hi_year = int(train_hi[:4])
    inner_fit_hi = f"{train_hi_year - 1}-12-31"
    inner_val_lo = f"{train_hi_year}-01-01"

    # Fit on inner-fit window
    try:
        Xfit, yfit, _ = _build_xy_expanded(panel, train_lo, inner_fit_hi, interaction_pairs)
    except ValueError:
        return ALPHAS[0]

    # Evaluate on inner-val window
    best_alpha, best_ic = ALPHAS[0], -np.inf
    sub_val = panel[(panel["date"] >= inner_val_lo) & (panel["date"] <= train_hi)]

    for alpha in ALPHAS:
        w = fl.elastic_net_fit(Xfit, yfit, alpha=alpha, l1_ratio=0.5)
        # Compute val Rank-IC using expanded features with same interaction_pairs
        ics = []
        for _, g in sub_val.groupby("date"):
            g = g.dropna(subset=["fwd_ret_5d"])
            if len(g) < 5:
                continue
            Xr = fl.rank_columns(g[FACTOR_COLS].to_numpy(float))
            Xexp = fl.expand_features(Xr, interaction_pairs)
            score = Xexp @ w
            ic = fl.rank_ic(score, g["fwd_ret_5d"].to_numpy(float))
            if not np.isnan(ic):
                ics.append(ic)
        val_ic = float(np.nanmean(ics)) if ics else float("nan")
        if not np.isnan(val_ic) and val_ic > best_ic:
            best_alpha, best_ic = alpha, val_ic

    return best_alpha


# ---------------------------------------------------------------------------
# Per-fold training (the main unit of work)
# ---------------------------------------------------------------------------

def train_fold(panel, fold):
    """Train one WFO fold.

    Args:
        panel: full DataFrame (may contain OOS rows — they are ignored).
        fold:  (train_lo, train_hi, oos_lo, oos_hi) strings.

    Returns:
        dict with keys: weights, alpha, interaction_pairs, feat_names.
        - weights: list[float] aligned to feat_names.
        - alpha: float selected via inner split.
        - interaction_pairs: list[list[int,int]] (serialisable).
        - feat_names: list[str].
    """
    train_lo, train_hi, _oos_lo, _oos_hi = fold

    # 1. Select interaction pairs using ONLY train window
    pairs = select_interactions(panel, FACTOR_COLS, train_lo, train_hi, k=5)

    # 2. Alpha selection via inner split (within train)
    alpha = _select_alpha_for_fold(panel, train_lo, train_hi, pairs)

    # 3. Full train fit with chosen alpha
    Xtr, ytr, _ = _build_xy_expanded(panel, train_lo, train_hi, pairs)
    w = fl.elastic_net_fit(Xtr, ytr, alpha=alpha, l1_ratio=0.5)

    feat_names = _build_feat_names(pairs)

    return {
        "weights": list(float(wi) for wi in w),
        "alpha": alpha,
        "interaction_pairs": [[int(i), int(j)] for i, j in pairs],
        "feat_names": feat_names,
    }


# ---------------------------------------------------------------------------
# Panel loader (extracted so tests can monkeypatch it)
# ---------------------------------------------------------------------------

def _load_panel():
    return pd.read_csv(PANEL, dtype={"symbol": str})


# ---------------------------------------------------------------------------
# Main entry point
# ---------------------------------------------------------------------------

def main():
    panel = _load_panel()
    os.makedirs(OUT_DIR, exist_ok=True)

    folds_out = []
    for fold in WFO_FOLDS:
        train_lo, train_hi, oos_lo, oos_hi = fold
        print(f"\n[fold] train={train_lo}..{train_hi}  OOS={oos_lo}..{oos_hi}")
        result = train_fold(panel, fold)
        folds_out.append({
            "train_lo": train_lo,
            "train_hi": train_hi,
            "oos_lo": oos_lo,
            "oos_hi": oos_hi,
            "weights": result["weights"],
            "alpha": result["alpha"],
            "interaction_pairs": result["interaction_pairs"],
            "feat_names": result["feat_names"],
        })
        print(f"  alpha={result['alpha']}  n_features={len(result['feat_names'])}")

    out = {"folds": folds_out}
    with open(WEIGHTS_NL, "w", encoding="utf-8") as fp:
        json.dump(out, fp, ensure_ascii=False, indent=2)
    print(f"\n-> {WEIGHTS_NL}")


if __name__ == "__main__":
    main()
