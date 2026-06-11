/// 平均秩（并列取平均，1 起）。
pub fn average_ranks(values: &[f64]) -> Vec<f64> {
    let n = values.len();
    let mut idx: Vec<usize> = (0..n).collect();
    idx.sort_by(|&a, &b| values[a].partial_cmp(&values[b]).unwrap_or(std::cmp::Ordering::Equal));
    let mut ranks = vec![0.0; n];
    let mut i = 0;
    while i < n {
        let mut j = i;
        while j + 1 < n && values[idx[j + 1]] == values[idx[i]] {
            j += 1;
        }
        let avg = (i + 1 + j + 1) as f64 / 2.0;
        for k in i..=j {
            ranks[idx[k]] = avg;
        }
        i = j + 1;
    }
    ranks
}

/// Pearson 相关：n≥2 且两侧方差 > 1e-12，否则 None。
pub fn pearson(x: &[f64], y: &[f64]) -> Option<f64> {
    if x.len() != y.len() || x.len() < 2 {
        return None;
    }
    let n = x.len() as f64;
    let mx = x.iter().sum::<f64>() / n;
    let my = y.iter().sum::<f64>() / n;
    let (mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0);
    for i in 0..x.len() {
        let (dx, dy) = (x[i] - mx, y[i] - my);
        sxx += dx * dx;
        syy += dy * dy;
        sxy += dx * dy;
    }
    if sxx <= 1e-12 || syy <= 1e-12 {
        return None;
    }
    Some(sxy / (sxx.sqrt() * syy.sqrt()))
}

/// Spearman = Pearson(平均秩, 平均秩)。
pub fn spearman(x: &[f64], y: &[f64]) -> Option<f64> {
    if x.len() != y.len() || x.len() < 2 {
        return None;
    }
    pearson(&average_ranks(x), &average_ranks(y))
}

/// 分层大小：基础 n/q，前 n%q 层 +1（升序因子值连续切层）。
pub fn layer_sizes(n: usize, q: usize) -> Vec<usize> {
    let base = n / q;
    let rem = n % q;
    (0..q).map(|i| base + usize::from(i < rem)).collect()
}

/// IC 衰减阶梯：dedup{max(h/4,1), max(h/2,1), h, 2h, 4h} 升序。
pub fn decay_ladder(h: usize) -> Vec<usize> {
    let mut v = vec![(h / 4).max(1), (h / 2).max(1), h, 2 * h, 4 * h];
    v.sort_unstable();
    v.dedup();
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn average_ranks_ties_take_mean() {
        assert_eq!(average_ranks(&[10.0, 20.0, 20.0, 30.0]), vec![1.0, 2.5, 2.5, 4.0]);
        assert_eq!(average_ranks(&[3.0, 1.0, 2.0]), vec![3.0, 1.0, 2.0]);
    }

    #[test]
    fn pearson_closed_form() {
        assert_relative_eq!(pearson(&[1.0, 2.0, 3.0], &[2.0, 4.0, 6.0]).unwrap(), 1.0, epsilon = 1e-12);
        assert_relative_eq!(pearson(&[1.0, 2.0, 3.0], &[6.0, 4.0, 2.0]).unwrap(), -1.0, epsilon = 1e-12);
        assert!(pearson(&[1.0, 1.0, 1.0], &[1.0, 2.0, 3.0]).is_none()); // 零方差
        assert!(pearson(&[1.0], &[1.0]).is_none()); // n<2
    }

    #[test]
    fn spearman_monotone_nonlinear_is_one() {
        assert_relative_eq!(
            spearman(&[1.0, 2.0, 3.0, 4.0], &[1.0, 10.0, 100.0, 1000.0]).unwrap(),
            1.0, epsilon = 1e-12
        );
        assert_relative_eq!(
            spearman(&[1.0, 2.0, 3.0, 4.0], &[1000.0, 100.0, 10.0, 1.0]).unwrap(),
            -1.0, epsilon = 1e-12
        );
    }

    #[test]
    fn layer_sizes_distributes_remainder_to_front() {
        assert_eq!(layer_sizes(11, 5), vec![3, 2, 2, 2, 2]);
        assert_eq!(layer_sizes(10, 5), vec![2, 2, 2, 2, 2]);
    }

    #[test]
    fn decay_ladder_dedups_and_sorts() {
        assert_eq!(decay_ladder(4), vec![1, 2, 4, 8, 16]);
        assert_eq!(decay_ladder(1), vec![1, 2, 4]); // max(…,1) 去重
        assert_eq!(decay_ladder(16), vec![4, 8, 16, 32, 64]);
    }
}
