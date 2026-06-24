"""TDD for eval_ridge_decile — rank-bucket return profile of the ridge signal.

Tests the bucketing primitives that pin the "objective fits broad order, not the
tail" diagnosis:
  - decile_means: ascending score → contiguous buckets (remainder front), per-bucket mean
  - top_rank_means: by descending score (rank 1 = top), mean over 1-based rank ranges
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import numpy as np

import eval_ridge_decile as dc


# ---------------------------------------------------------------------------
# decile_means(scores, rets, n_buckets)
# ---------------------------------------------------------------------------

def test_decile_means_two_even_buckets():
    out = dc.decile_means([1, 2, 3, 4], [1, 2, 3, 4], n_buckets=2)
    assert np.allclose(out, [1.5, 3.5])


def test_decile_means_remainder_goes_to_front_bucket():
    # 5 obs, 2 buckets → [[1,2,3],[4,5]] (larger chunk first, "余数前置")
    out = dc.decile_means([1, 2, 3, 4, 5], [1, 2, 3, 4, 5], n_buckets=2)
    assert np.allclose(out, [2.0, 4.5])


def test_decile_means_sorts_by_score_not_input_order():
    # rets follow scores; shuffled input must still bucket by score
    scores = [4, 1, 3, 2]
    rets = [40, 10, 30, 20]
    out = dc.decile_means(scores, rets, n_buckets=2)
    assert np.allclose(out, [15.0, 35.0])   # low-score bucket {1,2}->{10,20}; high {3,4}->{30,40}


def test_decile_means_monotone_when_signal_perfect():
    scores = list(range(10))
    rets = list(range(10))
    out = dc.decile_means(scores, rets, n_buckets=10)
    assert all(out[i] < out[i + 1] for i in range(9))


# ---------------------------------------------------------------------------
# top_rank_means(scores, rets, ranges)  — rank 1 = highest score
# ---------------------------------------------------------------------------

def test_top_rank_means_basic_ranges():
    scores = [10, 20, 30, 40, 50]
    rets = [1, 2, 3, 4, 5]
    out = dc.top_rank_means(scores, rets, [(1, 2), (3, 5)])
    assert np.allclose(out, [4.5, 2.0])   # ranks1-2 = rets{5,4}; ranks3-5 = {3,2,1}


def test_top_rank_means_clips_when_range_exceeds_n():
    scores = [10, 20, 30]
    rets = [1, 2, 3]
    out = dc.top_rank_means(scores, rets, [(1, 3), (4, 10)])
    assert abs(out[0] - 2.0) < 1e-12       # ranks1-3 = {3,2,1} mean 2.0
    assert np.isnan(out[1])                # ranks4-10 empty → NaN


def test_top_rank_means_top3_vs_next():
    # the crux comparison the experiment is built around
    scores = [9, 8, 7, 6, 5, 4, 3, 2, 1, 0]
    rets = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]   # inverted: top scores have LOW rets
    out = dc.top_rank_means(scores, rets, [(1, 3), (4, 10)])
    assert out[0] < out[1]                  # top-3 worse than 4-10 (tail anti-selected)
