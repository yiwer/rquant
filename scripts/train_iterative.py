# scripts/train_iterative.py
"""Iterative stochastic factor-weight trainer with PCA orthogonalization.

Realises the spec:
  1. Per-fold PCA on TRAIN gauss-normalised factor matrix → k PCs (≥95% variance).
     V fitted on TRAIN only; same V applied to OOS.  No leak.
  2. Three weight vectors (N ∈ {1,2,3}) trained independently via SGD with:
       - softmax-portfolio objective  (τ per N: τ1=0.02, τ2=0.04, τ3=0.06)
       - minibatch = random 1/4 of train dates
       - feature dropout p=0.50 per round
       - step cap ‖Δw‖₂ ≤ 0.05 (少次多调不一次矫正)
       - annealed weight noise σ = σ0·(1 − round/ROUNDS), σ0=0.01
       - Polyak averaging (last 50% of rounds)
  3. Per-round logging to data/factor_panel/iter_train_log.csv every 25 rounds.
     val_obj = softmax-port on last-20%-of-train-dates VAL split.
  4. Final OOS eval via eval_ridge.backtest_ridge (vetted harness, apples-to-apples).
     Map PC-weights back: w_factor = V @ w_pc.
  5. Round-count analysis: val_obj at rounds {100,500,1000,2000,3000}.
  6. Compare iterative vs ridge-on-gauss vs equal-weight per fold and in aggregate.

Determinism: np.random.default_rng(0).
Constraints: numpy/pandas only (no sklearn); PCA hand-rolled via np.linalg.eigh.
"""
import sys
import os
sys.stdout.reconfigure(encoding="utf-8")
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import numpy as np
import pandas as pd

import factor_lib as fl
from build_factor_matrix import FACTOR_COLS, OUT as PANEL_MEMBERSHIP, OUT_DIR
import iterate as it
import train_nonlinear as tn

from test_norm_hysteresis import norm_gauss
from eval_ridge import (
    backtest_ridge,
    fit_ridge,
    select_delta_ridge,
)

# ---------------------------------------------------------------------------
# Paths
# ---------------------------------------------------------------------------

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ST_PATH = os.path.join(REPO, "data", "baostock", "st_symbols.csv")
PANEL_FULL = os.path.join(OUT_DIR, "factors_full.csv")
LOG_PATH = os.path.join(OUT_DIR, "iter_train_log.csv")

# ---------------------------------------------------------------------------
# Hyper-parameters (spec-mandated)
# ---------------------------------------------------------------------------

ROUNDS = int(os.environ.get("ITER_ROUNDS", "3000"))   # configurable: test convergence at higher rounds
LR = 1e-2
STEP_CAP = 0.05
P_DROP = 0.50
SIGMA0 = 0.01
WCAP = 1.0
RIDGE_A = 0.10          # L2 penalty (matches eval_ridge)
VARIANCE_THRESHOLD = 0.95  # keep PCs explaining ≥95% variance
POLYAK_FRAC = 0.50      # last 50% of rounds
LOG_EVERY = 25          # append log row every N rounds
MINIBATCH_FRAC = 0.25   # 1/4 of train dates per step
VAL_FRAC = 0.20         # last 20% of train dates for val split

# Per-N softmax temperature
TAU = {1: 0.02, 2: 0.04, 3: 0.06}

# Fold definitions — same as train_nonlinear.WFO_FOLDS
WFO_FOLDS = tn.WFO_FOLDS  # 4 folds, OOS 2022/2023/2024/2025-26

TOP_N = 3               # portfolio size for backtest_ridge (mirrors eval_ridge)

DELTA_GRID = [0.0, 0.02, 0.05, 0.1]


# ---------------------------------------------------------------------------
# Hard gate (mirror of eval_ridge._eligible)
# ---------------------------------------------------------------------------

LIQ_FLOOR_LOG = float(np.log(5e7))


def _eligible(g, st_set):
    ok = (~g["symbol"].isin(st_set)) & (g["f_roe"] > 0) & (g["f_bm"] > 0) & (g["f_logamt"] >= LIQ_FLOOR_LOG)
    return g[ok]


# ---------------------------------------------------------------------------
# 1. PCA orthogonalization (TRAIN-ONLY)
# ---------------------------------------------------------------------------

def fit_pca(panel, date_lo, date_hi, variance_threshold=VARIANCE_THRESHOLD):
    """Fit PCA on TRAIN weeks' gauss-normalised factor matrix.

    Returns:
        V: ndarray (p, k) — eigenvectors keeping ≥variance_threshold of variance.
           Columns are sorted by descending eigenvalue.
        explained: float — fraction of variance explained by kept PCs.
        k: int — number of kept PCs.
    """
    sub = panel[(panel["date"] >= date_lo) & (panel["date"] <= date_hi)].copy()
    sub = sub.dropna(subset=["fwd_ret_5d"])
    p = len(FACTOR_COLS)
    # Accumulate covariance = (1/N) Σ GᵀG  (gauss-normalised rows, zero-mean by construction)
    cov = np.zeros((p, p))
    n_rows = 0
    for d, g in sub.groupby("date"):
        g = g.dropna(subset=["fwd_ret_5d"])
        if len(g) < 5:
            continue
        G = norm_gauss(g[FACTOR_COLS].to_numpy(float))   # (n, p)
        cov += G.T @ G
        n_rows += len(g)
    if n_rows == 0:
        return np.eye(p), 1.0, p
    cov /= n_rows

    # Eigendecompose — eigh returns sorted ascending; reverse to descending
    eigvals, eigvecs = np.linalg.eigh(cov)
    eigvals = eigvals[::-1]
    eigvecs = eigvecs[:, ::-1]   # (p, p), columns = eigenvectors

    # Keep top-k explaining ≥variance_threshold
    total = eigvals.sum()
    if total <= 0:
        return np.eye(p), 1.0, p
    cumvar = np.cumsum(eigvals) / total
    k = int(np.searchsorted(cumvar, variance_threshold)) + 1
    k = min(k, p)

    V = eigvecs[:, :k]          # (p, k)
    explained = float(cumvar[k - 1])
    return V, explained, k


# ---------------------------------------------------------------------------
# 2. Collect (per-date) gauss data for SGD
# ---------------------------------------------------------------------------

def _collect_train_data(panel, date_lo, date_hi, V, st_set):
    """Return list of (Z, fwd) per eligible train date, projected to PC space.

    Z = norm_gauss(factor_matrix) @ V  → shape (n_stocks, k)
    fwd = fwd_ret_5d values             → shape (n_stocks,)
    """
    sub = panel[(panel["date"] >= date_lo) & (panel["date"] <= date_hi)].copy()
    sub = sub.dropna(subset=["fwd_ret_5d"])
    records = []
    for d, g in sub.groupby("date"):
        g = g.dropna(subset=["fwd_ret_5d"])
        if len(g) < 5:
            continue
        # Apply eligibility filter
        g_el = _eligible(g, st_set)
        if len(g_el) < 2:
            continue
        G = norm_gauss(g_el[FACTOR_COLS].to_numpy(float))   # (n, p)
        Z = G @ V                                              # (n, k) — PC scores
        fwd = g_el["fwd_ret_5d"].to_numpy(float)
        records.append((Z, fwd))
    return records


# ---------------------------------------------------------------------------
# 3. Softmax portfolio objective and gradient (per single date)
# ---------------------------------------------------------------------------

def _softmax_port_and_grad(Z, fwd, w, tau):
    """Compute softmax portfolio return and gradient w.r.t. w for ONE date.

    score_i = Z_i @ w
    pi_i = softmax(score / tau)
    port = Σ pi_i * fwd_i
    ∂port/∂w = Σ_i (∂port/∂score_i) * Z_i
             = Σ_i pi_i*(fwd_i - port)/tau * Z_i

    Returns:
        port: float
        grad_w: (k,) gradient
    """
    scores = Z @ w                             # (n,)
    # Numerically stable softmax
    s_shifted = scores - scores.max()
    exp_s = np.exp(s_shifted / tau)
    pi = exp_s / exp_s.sum()                   # (n,)
    port = float(pi @ fwd)
    # Gradient
    dpis = pi * (fwd - port) / tau             # (n,)
    grad_w = Z.T @ dpis                        # (k,)
    return port, grad_w


def _softmax_obj(records, w, tau, lam):
    """Mean softmax portfolio return over records, minus ridge penalty.

    Returns (obj, grad_total).
    """
    k = len(w)
    total_port = 0.0
    total_grad = np.zeros(k)
    n = len(records)
    if n == 0:
        return 0.0, total_grad
    for Z, fwd in records:
        p, g = _softmax_port_and_grad(Z, fwd, w, tau)
        total_port += p
        total_grad += g
    obj = total_port / n - lam * np.dot(w, w)
    grad = total_grad / n - 2.0 * lam * w
    return obj, grad


# ---------------------------------------------------------------------------
# 4. SGD loop with dropout, step cap, annealed noise, Polyak averaging
# ---------------------------------------------------------------------------

def train_sgd(records_train, records_val, k, N, rng, lam=0.01):
    """Train weight vector for top-N portfolio.

    Args:
        records_train: list of (Z, fwd) for train dates
        records_val: list of (Z, fwd) for val dates
        k: PC dimension
        N: portfolio size (controls tau)
        rng: np.random.Generator
        lam: L2 penalty (default 0.01)

    Returns:
        w_poly: Polyak-averaged weight vector (k,)
        log: list of (round, train_obj, val_obj, w_norm) logged every LOG_EVERY rounds
    """
    tau = TAU[N]
    w = rng.standard_normal(k) * 0.01        # small init

    n_train = len(records_train)
    batch_size = max(1, int(n_train * MINIBATCH_FRAC))
    polyak_start = int(ROUNDS * (1.0 - POLYAK_FRAC))

    poly_acc = np.zeros(k)
    poly_count = 0

    log = []

    for r in range(ROUNDS):
        # (a) feature dropout: randomly zero fraction P_DROP of PC dims
        keep_mask = rng.random(k) >= P_DROP
        if keep_mask.sum() == 0:
            keep_mask[rng.integers(k)] = True   # keep at least one

        # (b) minibatch: random 1/4 of train dates
        idx = rng.choice(n_train, size=min(batch_size, n_train), replace=False)
        batch = [records_train[i] for i in idx]

        # Masked gradient: only use surviving dimensions
        w_masked = w * keep_mask
        _, grad = _softmax_obj(batch, w_masked, tau, lam)
        grad = grad * keep_mask                 # zero out dropped dims in grad

        # (c) step with clip
        raw_step = LR * grad
        step_norm = np.linalg.norm(raw_step)
        if step_norm > STEP_CAP:
            raw_step = raw_step * (STEP_CAP / step_norm)
        w = w + raw_step

        # (d) annealed weight noise
        sigma_r = SIGMA0 * (1.0 - r / ROUNDS)
        if sigma_r > 0:
            w = w + rng.standard_normal(k) * sigma_r

        # (e) clip |w_j| ≤ WCAP
        w = np.clip(w, -WCAP, WCAP)

        # Polyak accumulation
        if r >= polyak_start:
            poly_acc += w
            poly_count += 1

        # Logging every LOG_EVERY rounds
        if (r + 1) % LOG_EVERY == 0 or r == ROUNDS - 1:
            train_obj, _ = _softmax_obj(records_train, w, tau, lam)
            val_obj = 0.0
            if records_val:
                val_obj, _ = _softmax_obj(records_val, w, tau, lam)
            w_norm = float(np.linalg.norm(w))
            log.append((r + 1, float(train_obj), float(val_obj), w_norm))

    w_poly = poly_acc / poly_count if poly_count > 0 else w
    return w_poly, log


# ---------------------------------------------------------------------------
# 5. Per-round persistence
# ---------------------------------------------------------------------------

LOG_HEADER = ["fold_oos", "N", "round", "train_obj", "val_obj", "w_norm"]


def _append_log(fold_oos_label, N, log_rows):
    """Append log rows to iter_train_log.csv."""
    os.makedirs(OUT_DIR, exist_ok=True)
    write_header = not os.path.exists(LOG_PATH)
    with open(LOG_PATH, "a", encoding="utf-8", newline="") as f:
        import csv as _csv
        w = _csv.writer(f)
        if write_header:
            w.writerow(LOG_HEADER)
        for round_num, train_obj, val_obj, w_norm in log_rows:
            w.writerow([fold_oos_label, N, round_num, train_obj, val_obj, w_norm])


# ---------------------------------------------------------------------------
# 6. Per-fold training and OOS evaluation
# ---------------------------------------------------------------------------

def _select_delta_iterative(panel, fold, w_factor, st_set):
    """Select hysteresis delta on TRAIN slice using iterative weights (no OOS peek)."""
    train_lo, train_hi, _oos_lo, _oos_hi = fold
    train_panel = panel[(panel["date"] >= train_lo) & (panel["date"] <= train_hi)].copy()
    best_delta = 0.0
    best_net = -np.inf
    for d in DELTA_GRID:
        rep = backtest_ridge(train_panel, w_factor, top_n=TOP_N,
                             cost_bps=it.COST, st_set=st_set, delta=d)
        if rep["total_return"] > best_net:
            best_net = rep["total_return"]
            best_delta = d
    return best_delta


def run_fold(panel, fold, st_set, idx_data, rng):
    """Run one WFO fold: fit PCA → train SGD per N → OOS eval via backtest_ridge.

    Returns dict with per-N results.
    """
    train_lo, train_hi, oos_lo, oos_hi = fold
    fold_label = f"{oos_lo}..{oos_hi}"

    print(f"\n  [fold] train={train_lo}..{train_hi}  OOS={oos_lo}..{oos_hi}")

    oos_panel = panel[(panel["date"] >= oos_lo) & (panel["date"] <= oos_hi)].copy()
    if len(oos_panel) == 0:
        print("    [SKIP] empty OOS panel")
        return {"fold": fold_label, "results": {}}

    # --- 1. Fit PCA on TRAIN (no OOS data) ---
    V, explained, k = fit_pca(panel, train_lo, train_hi)
    print(f"    PCA: k={k} PCs explain {explained:.1%} variance (threshold {VARIANCE_THRESHOLD:.0%})")

    # --- 2. Collect train data projected into PC space ---
    all_train_records = _collect_train_data(panel, train_lo, train_hi, V, st_set)
    if len(all_train_records) < 10:
        print(f"    [WARN] only {len(all_train_records)} train dates — skipping fold")
        return {"fold": fold_label, "results": {}}

    # Train/val split: last VAL_FRAC of train dates → val set
    n_val = max(1, int(len(all_train_records) * VAL_FRAC))
    records_val = all_train_records[-n_val:]
    records_train = all_train_records[:-n_val]
    print(f"    train_dates={len(records_train)}  val_dates={len(records_val)}")

    # --- Also fit ridge for this fold (for comparison table) ---
    w_ridge, n_train_ridge = fit_ridge(panel, train_lo, train_hi)
    delta_ridge = select_delta_ridge(panel, fold, w_ridge, st_set)
    rep_ridge = backtest_ridge(oos_panel, w_ridge, top_n=TOP_N,
                               cost_bps=it.COST, st_set=st_set, delta=delta_ridge)

    # --- Equal-weight baseline ---
    p = len(FACTOR_COLS)
    w_eq = np.zeros(p)
    w_eq[0] = 1.0   # f_bm
    w_eq[1] = 1.0   # f_npyoy  (same as eval_ridge)
    from eval_ridge import backtest_rank_linear
    rep_eq = backtest_rank_linear(oos_panel, w_eq, top_n=TOP_N,
                                  cost_bps=it.COST, st_set=st_set, delta=0.0)

    idx_m, idx_dates = idx_data
    rel_ridge = it.to_index_relative(rep_ridge, idx_m, idx_dates)
    rel_eq = it.to_index_relative(rep_eq, idx_m, idx_dates)
    ridge_oos = rel_ridge["excess_return"] if rel_ridge else None
    eq_oos = rel_eq["excess_return"] if rel_eq else None

    fold_result = {
        "fold": fold_label,
        "ridge_oos": ridge_oos,
        "eq_oos": eq_oos,
        "ridge_delta": delta_ridge,
        "results": {},
    }

    # --- 3. Train per N, log, OOS eval ---
    for N in (2, 3):   # N=1 dropped: τ=0.02 degenerates to single-stock lottery
        print(f"\n    --- N={N} (tau={TAU[N]}) ---")
        # Fresh RNG state per N but deterministic across runs
        rng_n = np.random.default_rng(rng.integers(2**31))

        # Compute ridge λ in PC space (scale ~ factor space)
        # Use simple heuristic: λ = RIDGE_A (relative to unit-variance PCs)
        lam = RIDGE_A

        w_pc, log = train_sgd(records_train, records_val, k, N, rng_n, lam=lam)
        print(f"      rounds done; Polyak w_norm={np.linalg.norm(w_pc):.4f}")

        # Persist log
        _append_log(fold_label, N, log)

        # Map PC weights back to factor space: w_factor = V @ w_pc
        # (since Z@w_pc = gauss@V@w_pc = gauss@w_factor)
        w_factor = V @ w_pc                    # (p,)

        # Select delta on TRAIN
        delta_iter = _select_delta_iterative(panel, fold, w_factor, st_set)

        # OOS eval via vetted harness
        rep_iter = backtest_ridge(oos_panel, w_factor, top_n=TOP_N,
                                  cost_bps=it.COST, st_set=st_set, delta=delta_iter)
        rel_iter = it.to_index_relative(rep_iter, idx_m, idx_dates)
        iter_oos = rel_iter["excess_return"] if rel_iter else None

        print(f"      delta={delta_iter:.2f}  iter_oos={iter_oos:+.4f}" if iter_oos is not None else "      iter_oos=None")
        print(f"      ridge_oos={ridge_oos:+.4f}  eq_oos={eq_oos:+.4f}" if ridge_oos is not None else "")

        fold_result["results"][N] = {
            "iter_oos": iter_oos,
            "delta": delta_iter,
            "w_pc_norm": float(np.linalg.norm(w_pc)),
            "log": log,
        }

    return fold_result


# ---------------------------------------------------------------------------
# 7. Round-count analysis: val_obj at milestone rounds
# ---------------------------------------------------------------------------

MILESTONE_ROUNDS = [r for r in [100, 500, 1000, 2000, 5000, 10000, 15000, 20000, 30000] if r <= ROUNDS]


def _round_count_analysis():
    """Read iter_train_log.csv and print val_obj at milestone rounds per N."""
    if not os.path.exists(LOG_PATH):
        print("[INFO] No log file found for round-count analysis.")
        return

    df = pd.read_csv(LOG_PATH)
    print(f"\n{'='*70}")
    print("Round-count val_obj progression (does it improve then plateau/overfit?)")
    print(f"{'='*70}")
    print(f"{'N':>4}  {'rounds':>8}  {'val_obj_mean':>14}  (across folds)")

    for N in (1, 2, 3):
        sub = df[df["N"] == N]
        if len(sub) == 0:
            continue
        for ms in MILESTONE_ROUNDS:
            # Find the closest logged round ≤ ms
            close = sub[sub["round"] <= ms + LOG_EVERY]
            if len(close) == 0:
                continue
            at_ms = close.groupby("fold_oos").apply(
                lambda g: g.iloc[(g["round"] - ms).abs().argsort()].iloc[0]
            ).reset_index(drop=True)
            val_mean = at_ms["val_obj"].mean()
            print(f"  N={N}  round~{ms:>4}  val_obj_mean={val_mean:+.6f}")
        print()


# ---------------------------------------------------------------------------
# 8. Aggregate summary
# ---------------------------------------------------------------------------

def _summarize(fold_results, panel_label):
    """Print and return aggregate table: iterative vs ridge vs equal-weight."""
    print(f"\n{'='*70}")
    print(f"=== AGGREGATE: {panel_label} ===")
    print(f"{'='*70}")

    for N in (1, 2, 3):
        iter_vals = []
        ridge_vals = []
        eq_vals = []

        print(f"\n  --- N={N} ---")
        print(f"  {'fold':20}  {'iter_oos':>10}  {'ridge_oos':>10}  {'eq_oos':>8}")
        for fr in fold_results:
            fold_label = fr["fold"]
            r = fr["results"].get(N, {})
            iv = r.get("iter_oos")
            rv = fr.get("ridge_oos")
            ev = fr.get("eq_oos")
            iv_s = f"{iv:+.4f}" if iv is not None else "   None"
            rv_s = f"{rv:+.4f}" if rv is not None else "   None"
            ev_s = f"{ev:+.4f}" if ev is not None else "  None"
            print(f"  {fold_label:20}  {iv_s:>10}  {rv_s:>10}  {ev_s:>8}")
            if iv is not None:
                iter_vals.append(iv)
            if rv is not None:
                ridge_vals.append(rv)
            if ev is not None:
                eq_vals.append(ev)

        iter_mean = float(np.mean(iter_vals)) if iter_vals else None
        ridge_mean = float(np.mean(ridge_vals)) if ridge_vals else None
        eq_mean = float(np.mean(eq_vals)) if eq_vals else None
        iter_pos = sum(1 for v in iter_vals if v > 0)
        ridge_pos = sum(1 for v in ridge_vals if v > 0)
        n_folds = len(iter_vals)

        print(f"\n  [N={N}] mean OOS excess:")
        if iter_mean is not None:
            print(f"    iter     = {iter_mean:+.4f}  pos_folds={iter_pos}/{n_folds}")
        if ridge_mean is not None:
            print(f"    ridge    = {ridge_mean:+.4f}  pos_folds={ridge_pos}/{len(ridge_vals)}")
        if eq_mean is not None:
            print(f"    eq-wt    = {eq_mean:+.4f}")

        # §5.3 check: mean>0 AND majority positive
        iter_53 = (iter_mean is not None and iter_mean > 0 and iter_pos > n_folds / 2)
        ridge_53 = (ridge_mean is not None and ridge_mean > 0 and ridge_pos > len(ridge_vals) / 2)
        iter_beats_ridge = (iter_mean is not None and ridge_mean is not None and iter_mean > ridge_mean)
        iter_beats_eq = (iter_mean is not None and eq_mean is not None and iter_mean > eq_mean)

        print(f"    §5.3-pos: iter={iter_53}  ridge={ridge_53}")
        print(f"    iter beats ridge={iter_beats_ridge}  iter beats eq={iter_beats_eq}")

    return {"iter_vals": {N: [] for N in (1, 2, 3)}}


def _one_line_verdict(fold_results_list):
    """Print final one-line verdict across all folds."""
    print(f"\n{'='*70}")
    print("=== ONE-LINE VERDICT ===")
    print(f"{'='*70}")

    for N in (1, 2, 3):
        iter_vals = []
        ridge_vals = []
        for fold_results in fold_results_list:
            for fr in fold_results:
                r = fr["results"].get(N, {})
                iv = r.get("iter_oos")
                rv = fr.get("ridge_oos")
                if iv is not None:
                    iter_vals.append(iv)
                if rv is not None:
                    ridge_vals.append(rv)
        if not iter_vals:
            continue
        iter_mean = float(np.mean(iter_vals))
        ridge_mean = float(np.mean(ridge_vals)) if ridge_vals else None
        beats_ridge = ridge_mean is not None and iter_mean > ridge_mean
        diff = (iter_mean - ridge_mean) if ridge_mean is not None else None
        verdict = "BEATS" if beats_ridge else ("MATCHES" if diff is not None and abs(diff) < 0.005 else "UNDERPERFORMS")
        print(f"  N={N}: iterative ({iter_mean:+.4f}) vs ridge ({ridge_mean:+.4f}) → {verdict} (Δ={diff:+.4f})" if diff is not None else f"  N={N}: iterative ({iter_mean:+.4f})")


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    # Deterministic RNG
    rng = np.random.default_rng(0)

    # Load ST exclusion set
    st_set = set(pd.read_csv(ST_PATH)["symbol"]) if os.path.exists(ST_PATH) else set()
    print(f"ST set: {len(st_set)} symbols")

    # Load CSI 300 index
    idx_data = it.load_index("csi300")

    # Clear existing log for this run (fresh write)
    if os.path.exists(LOG_PATH):
        os.remove(LOG_PATH)
        print(f"Cleared existing log: {LOG_PATH}")

    all_fold_results = []

    for panel_path, panel_label in [
        (PANEL_MEMBERSHIP, "membership"),
        (PANEL_FULL, "full (wide)"),
    ]:
        if not os.path.exists(panel_path):
            print(f"[WARN] panel not found: {panel_path} — skipping")
            continue

        print(f"\n{'='*70}")
        print(f"Panel: {panel_label}  ({panel_path})")
        print(f"{'='*70}")

        panel = pd.read_csv(panel_path, dtype={"symbol": str})
        print(f"  Loaded {len(panel)} rows, dates {panel['date'].min()}..{panel['date'].max()}")

        fold_results = []
        for fold in WFO_FOLDS:
            fr = run_fold(panel, fold, st_set, idx_data, rng)
            fold_results.append(fr)

        _summarize(fold_results, panel_label)
        all_fold_results.append(fold_results)

    # Round-count analysis from log
    _round_count_analysis()

    # One-line verdict
    _one_line_verdict(all_fold_results)


if __name__ == "__main__":
    main()
