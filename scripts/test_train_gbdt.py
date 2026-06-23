"""Tests for train_gbdt.py — synthetic panel, no real data/ required."""
import numpy as np
import pandas as pd
import train_gbdt as tg
from build_factor_matrix import FACTOR_COLS


def _toy(seed=0, extra_oos=False):
    rng = np.random.default_rng(seed)
    rows = []
    for d in pd.bdate_range("2018-01-02", "2021-12-31", freq="5B").strftime("%Y-%m-%d"):
        for s in range(60):
            x = rng.normal(size=len(FACTOR_COLS))
            fwd = 1.2 * x[0] - 0.8 * x[3] + rng.normal(scale=0.5)
            rows.append([d, f"s{s}", *x, fwd])
    if extra_oos:  # 2022 OOS：反向极端信号，若被读到会改变模型
        for d in pd.bdate_range("2022-01-03", "2022-12-31", freq="5B").strftime("%Y-%m-%d"):
            for s in range(60):
                x = rng.normal(size=len(FACTOR_COLS))
                fwd = -9 * x[0] + rng.normal(scale=0.1)
                rows.append([d, f"s{s}", *x, fwd])
    return pd.DataFrame(rows, columns=["date", "symbol", *FACTOR_COLS, "fwd_ret_5d"])


FOLD = ("2018-01-02", "2021-12-31", "2022-01-03", "2022-12-31")


def test_ensemble_size_and_predict_shape():
    models = tg.train_fold_gbdt(_toy(), FOLD)
    assert len(models) == len(tg.ENSEMBLE_SEEDS)
    Xt = np.random.default_rng(1).random((20, len(FACTOR_COLS)))
    assert tg.ensemble_predict(models, Xt).shape == (20,)


def test_training_does_not_read_oos():
    Xt = np.random.default_rng(2).random((30, len(FACTOR_COLS)))
    p1 = tg.ensemble_predict(tg.train_fold_gbdt(_toy(extra_oos=False), FOLD), Xt)
    p2 = tg.ensemble_predict(tg.train_fold_gbdt(_toy(extra_oos=True), FOLD), Xt)
    assert np.allclose(p1, p2)  # OOS 行不改变 fold 模型（train 切片 + 确定性种子）
