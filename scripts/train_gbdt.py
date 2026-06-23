"""GBDT 多种子集成 WFO 训练：每折 LightGBM multi-seed ensemble。

锚定扩展 WFO（同 train_nonlinear.py）：
- train 固定从 2018-01-02 起，train_hi 逐年推进。
- OOS = 次年全年；OOS 窗口仅用于定义折边界，绝不参与拟合或早停。
- 内层早停：末年作为 val（在 train 内部），不接触 OOS。

产出: data/factor_panel/gbdt_models/fold{i}_seed{s}.txt + gbdt_meta.json
"""
import sys
sys.stdout.reconfigure(encoding="utf-8")
import os
import json
import numpy as np
import pandas as pd
import lightgbm as lgb

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import factor_lib as fl
from build_factor_matrix import FACTOR_COLS, OUT as PANEL, OUT_DIR

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

# Anchored-expanding WFO folds: (train_lo, train_hi, oos_lo, oos_hi)
# Mirrors WFO_FOLDS in train_nonlinear.py exactly.
WFO_FOLDS = [
    ("2018-01-02", "2021-12-31", "2022-01-02", "2022-12-31"),
    ("2018-01-02", "2022-12-31", "2023-01-02", "2023-12-31"),
    ("2018-01-02", "2023-12-31", "2024-01-02", "2024-12-31"),
    ("2018-01-02", "2024-12-31", "2025-01-02", "2026-06-30"),
]

ENSEMBLE_SEEDS = [0, 1, 2, 3, 4]

# Regularised LightGBM base params (no seed — added per member in train_fold_gbdt)
GBDT_PARAMS = {
    "objective": "regression",
    "num_leaves": 31,
    "max_depth": 5,
    "learning_rate": 0.03,
    "min_child_samples": 200,
    "feature_fraction": 0.7,
    "bagging_fraction": 0.7,
    "bagging_freq": 1,
    "lambda_l1": 1.0,
    "lambda_l2": 1.0,
    "n_estimators": 300,
    "deterministic": True,
    "force_row_wise": True,
    "num_threads": 1,
    "verbose": -1,
}

GBDT_MODELS_DIR = os.path.join(OUT_DIR, "gbdt_models")
GBDT_META = os.path.join(GBDT_MODELS_DIR, "gbdt_meta.json")


# ---------------------------------------------------------------------------
# Feature / label construction
# ---------------------------------------------------------------------------

def build_xy_gbdt(panel, date_lo, date_hi):
    """Slice panel to [date_lo, date_hi], compute per-date rank features and rank target.

    Args:
        panel:    DataFrame with columns: date, symbol, *FACTOR_COLS, fwd_ret_5d.
        date_lo:  inclusive lower date string.
        date_hi:  inclusive upper date string.

    Returns:
        (X, y): np.ndarray of shape (N, len(FACTOR_COLS)), (N,)
        - X: per-date cross-sectional rank of each factor column (rank_columns).
        - y: per-date cross-sectional rank of fwd_ret_5d (cross_sectional_rank).
        Rows where fwd_ret_5d is NaN are dropped; dates with <5 valid rows are skipped.

    Raises:
        ValueError if no valid rows remain after slicing and dropping.
    """
    sub = panel[(panel["date"] >= date_lo) & (panel["date"] <= date_hi)]
    Xs, ys = [], []
    for _d, g in sub.groupby("date"):
        g = g.dropna(subset=["fwd_ret_5d"])
        if len(g) < 5:
            continue
        Xr = fl.rank_columns(g[FACTOR_COLS].to_numpy(float))
        yr = fl.cross_sectional_rank(g["fwd_ret_5d"].to_numpy(float))
        Xs.append(Xr)
        ys.append(yr)
    if not Xs:
        raise ValueError(f"No valid rows in [{date_lo}, {date_hi}]")
    return np.vstack(Xs), np.concatenate(ys)


# ---------------------------------------------------------------------------
# Per-fold training
# ---------------------------------------------------------------------------

def train_fold_gbdt(panel, fold):
    """Train one WFO fold: K LightGBM boosters (one per seed in ENSEMBLE_SEEDS).

    The panel is sliced to [train_lo, train_hi] BEFORE any computation.
    OOS dates from the fold tuple are used ONLY to define boundaries;
    they are never read or passed to the model.

    Inner early-stopping split:
        - fit window:  [train_lo, train_hi_year-1 end]
        - val window:  [train_hi_year start, train_hi]
    Both halves are strictly inside train; OOS is never touched.

    Args:
        panel: full DataFrame (may contain OOS rows — they are ignored).
        fold:  (train_lo, train_hi, oos_lo, oos_hi) strings.

    Returns:
        list of K lgb.Booster objects, one per seed in ENSEMBLE_SEEDS.
    """
    train_lo, train_hi, _oos_lo, _oos_hi = fold

    # Slice panel to train window ONLY — OOS rows are discarded here.
    train_panel = panel[(panel["date"] >= train_lo) & (panel["date"] <= train_hi)]

    # Inner split: last year of train window = val for early stopping.
    train_hi_year = int(train_hi[:4])
    inner_fit_hi = f"{train_hi_year - 1}-12-31"
    inner_val_lo = f"{train_hi_year}-01-01"

    # Build (X, y) for inner fit window (all within train)
    Xfit, yfit = build_xy_gbdt(train_panel, train_lo, inner_fit_hi)
    # Build (X, y) for inner val window (all within train)
    Xval, yval = build_xy_gbdt(train_panel, inner_val_lo, train_hi)

    # Build (X, y) for full train window (used for final model)
    Xtr, ytr = build_xy_gbdt(train_panel, train_lo, train_hi)

    # Extract num_boost_round from GBDT_PARAMS (sklearn alias; lgb.train uses num_boost_round arg)
    num_boost_round = GBDT_PARAMS["n_estimators"]
    early_stop_rounds = 30

    # First pass: determine best iteration via inner split (seed 0).
    # Fresh Dataset objects per call — LightGBM forbids changing data_random_seed
    # after a Dataset handle is constructed, so we must not reuse Dataset objects
    # across seeds.
    probe_params = {k: v for k, v in GBDT_PARAMS.items() if k != "n_estimators"}
    probe_params.update({
        "seed": 0,
        "feature_fraction_seed": 0,
        "bagging_seed": 0,
        "data_random_seed": 0,
    })
    lgb_train_inner = lgb.Dataset(Xfit, label=yfit, params=probe_params, free_raw_data=False)
    lgb_val_inner = lgb.Dataset(Xval, label=yval, reference=lgb_train_inner, free_raw_data=False)
    callbacks_inner = [lgb.early_stopping(early_stop_rounds, verbose=False), lgb.log_evaluation(-1)]
    probe_model = lgb.train(
        probe_params,
        lgb_train_inner,
        num_boost_round=num_boost_round,
        valid_sets=[lgb_val_inner],
        callbacks=callbacks_inner,
    )
    best_iter = probe_model.best_iteration if probe_model.best_iteration > 0 else num_boost_round

    # Train K members on full train data for best_iter rounds, each with a distinct seed.
    # Create fresh Dataset objects per seed to avoid LightGBM's handle-mutation restriction.
    models = []
    for seed in ENSEMBLE_SEEDS:
        params = {k: v for k, v in GBDT_PARAMS.items() if k != "n_estimators"}
        params.update({
            "seed": seed,
            "feature_fraction_seed": seed,
            "bagging_seed": seed,
            "data_random_seed": seed,
        })
        lgb_train_full = lgb.Dataset(Xtr, label=ytr, params=params, free_raw_data=False)
        booster = lgb.train(
            params,
            lgb_train_full,
            num_boost_round=best_iter,
            callbacks=[lgb.log_evaluation(-1)],
        )
        models.append(booster)

    return models


# ---------------------------------------------------------------------------
# Ensemble prediction
# ---------------------------------------------------------------------------

def ensemble_predict(models, Xrank):
    """Mean prediction across K boosters.

    Args:
        models: list of lgb.Booster objects.
        Xrank:  np.ndarray of shape (N, len(FACTOR_COLS)) — already rank-transformed.

    Returns:
        np.ndarray of shape (N,): mean of individual booster predictions.
    """
    preds = np.stack([m.predict(Xrank) for m in models], axis=0)
    return preds.mean(axis=0)


# ---------------------------------------------------------------------------
# Panel loader (extracted so tests can monkeypatch it)
# ---------------------------------------------------------------------------

def _load_panel():
    return pd.read_csv(PANEL, dtype={"symbol": str})


# ---------------------------------------------------------------------------
# Main entry point
# ---------------------------------------------------------------------------

def main():
    """Train all WFO folds, save boosters and meta JSON."""
    panel = _load_panel()
    os.makedirs(GBDT_MODELS_DIR, exist_ok=True)

    folds_meta = []
    for i, fold in enumerate(WFO_FOLDS):
        train_lo, train_hi, oos_lo, oos_hi = fold
        print(f"\n[fold {i}] train={train_lo}..{train_hi}  OOS={oos_lo}..{oos_hi}")
        models = train_fold_gbdt(panel, fold)
        for j, (seed, booster) in enumerate(zip(ENSEMBLE_SEEDS, models)):
            path = os.path.join(GBDT_MODELS_DIR, f"fold{i}_seed{seed}.txt")
            booster.save_model(path)
            print(f"  saved {path}")
        folds_meta.append({
            "fold": i,
            "train_lo": train_lo,
            "train_hi": train_hi,
            "oos_lo": oos_lo,
            "oos_hi": oos_hi,
            "seeds": ENSEMBLE_SEEDS,
        })

    with open(GBDT_META, "w", encoding="utf-8") as fp:
        json.dump({"folds": folds_meta}, fp, ensure_ascii=False, indent=2)
    print(f"\n-> {GBDT_META}")


if __name__ == "__main__":
    main()
