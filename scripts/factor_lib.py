"""轻量因子管线共享纯计算：截面排名归一 / Elastic-Net / Rank-IC / 线性分。零 IO，零外部依赖（仅 numpy）。"""
import numpy as np


def cross_sectional_rank(values):
    """1D 百分位排名 ∈[0,1]，并列取平均名次，NaN→0.5（截面中位）。单元素→0.5。"""
    v = np.asarray(values, dtype=float)
    out = np.full(v.shape, 0.5)
    mask = ~np.isnan(v)
    m = int(mask.sum())
    if m <= 1:
        return out
    x = v[mask]
    order = np.argsort(x, kind="mergesort")
    ranks = np.empty(m)
    sx = x[order]
    i = 0
    while i < m:                       # 并列取平均名次（0-based）
        j = i
        while j + 1 < m and sx[j + 1] == sx[i]:
            j += 1
        ranks[order[i:j + 1]] = (i + j) / 2.0
        i = j + 1
    out[mask] = ranks / (m - 1)        # 归一到 [0,1]
    return out


def rank_columns(X):
    """对 2D 矩阵每列做 cross_sectional_rank。"""
    X = np.asarray(X, dtype=float)
    return np.column_stack([cross_sectional_rank(X[:, j]) for j in range(X.shape[1])])


def rank_ic(scores, fwd):
    """Spearman = 两者截面排名的 Pearson 相关；<2 有效点→nan。"""
    s = np.asarray(scores, float); f = np.asarray(fwd, float)
    mask = ~(np.isnan(s) | np.isnan(f))
    if mask.sum() < 2:
        return float("nan")
    rs = cross_sectional_rank(s[mask]); rf = cross_sectional_rank(f[mask])
    if rs.std() == 0 or rf.std() == 0:
        return float("nan")
    return float(np.corrcoef(rs, rf)[0, 1])


def elastic_net_fit(X, y, alpha, l1_ratio=0.5, max_iter=1000, tol=1e-7):
    """坐标下降解 Elastic-Net（中心化，无截距）。
    min (1/2n)‖yc−Xc w‖² + alpha(l1_ratio‖w‖₁ + (1−l1_ratio)/2‖w‖₂²)。"""
    X = np.asarray(X, float); y = np.asarray(y, float)
    n, p = X.shape
    Xc = X - X.mean(0); yc = y - y.mean()
    col_ss = (Xc ** 2).sum(0)          # 每列平方和
    w = np.zeros(p)
    r = yc.copy()                      # 残差 = yc − Xc w（w=0 起）
    l1 = alpha * l1_ratio
    l2 = alpha * (1.0 - l1_ratio)
    for _ in range(max_iter):
        w_max = 0.0
        for j in range(p):
            if col_ss[j] == 0:
                continue
            rho = Xc[:, j] @ r / n + (col_ss[j] / n) * w[j]
            denom = col_ss[j] / n + l2
            new = np.sign(rho) * max(abs(rho) - l1, 0.0) / denom
            if new != w[j]:
                r += Xc[:, j] * (w[j] - new)   # 增量更新残差
                w_max = max(w_max, abs(new - w[j]))
                w[j] = new
        if w_max < tol:
            break
    return w


def linear_score(Xrank, w):
    """线性打分 = 排名矩阵 · 权重。"""
    return np.asarray(Xrank, float) @ np.asarray(w, float)
