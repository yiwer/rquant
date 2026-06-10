use crate::backtest::metrics::{signal_stat, SignalStat};
use crate::data::bar::Bar;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FoldMetrics {
    pub from: NaiveDateTime,
    pub to: NaiveDateTime,
    pub stat: SignalStat,
    pub buy_and_hold: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalkForward {
    pub folds: Vec<FoldMetrics>,
    pub positive_folds: usize,
    pub worst_mean_net: f64,
}

/// 固定树滚动分折：决策点按索引等分 k 个连续折（空索引段省略）。
/// nets_per_point\[i\] = 第 i 点的参与净收益（未参与/未计分=None），与 primary_slice 一一对齐。
pub fn walk_forward(nets_per_point: &[Option<f64>], primary_slice: &[Bar], k: usize) -> WalkForward {
    let n = nets_per_point.len().min(primary_slice.len());
    let mut folds = Vec::new();
    for j in 0..k {
        let lo = j * n / k;
        let hi = (j + 1) * n / k;
        if hi <= lo {
            continue;
        }
        let nets: Vec<f64> = nets_per_point[lo..hi].iter().flatten().copied().collect();
        let bh = if primary_slice[lo].open > 0.0 {
            primary_slice[hi - 1].close / primary_slice[lo].open - 1.0
        } else {
            0.0
        };
        folds.push(FoldMetrics {
            from: primary_slice[lo].time,
            to: primary_slice[hi - 1].time,
            stat: signal_stat(&nets),
            buy_and_hold: bh,
        });
    }
    let positive_folds = folds.iter().filter(|f| f.stat.count > 0 && f.stat.mean_net > 0.0).count();
    let worst = folds.iter().filter(|f| f.stat.count > 0).map(|f| f.stat.mean_net).fold(f64::INFINITY, f64::min);
    let worst_mean_net = if worst.is_finite() { worst } else { 0.0 };
    WalkForward { folds, positive_folds, worst_mean_net }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::bar::Bar;
    use chrono::NaiveDate;

    fn bars(n: usize) -> Vec<Bar> {
        let base = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        (0..n).map(|i| Bar {
            time: base + chrono::Duration::minutes(i as i64 * 15),
            open: 10.0 + i as f64, high: 13.0 + i as f64, low: 10.0 + i as f64,
            close: 12.5 + i as f64, volume: 1.0,
        }).collect()
    }

    #[test]
    fn three_folds_known_values() {
        let p = bars(9);
        let nets = vec![
            Some(0.01), None, Some(0.03),       // fold0: mean 0.02
            None, None, None,                   // fold1: count 0
            Some(-0.01), Some(0.02), None,      // fold2: mean 0.005
        ];
        let wf = walk_forward(&nets, &p, 3);
        assert_eq!(wf.folds.len(), 3);
        assert_eq!(wf.folds[0].stat.count, 2);
        assert!((wf.folds[0].stat.mean_net - 0.02).abs() < 1e-12);
        // bh fold0 = close[2]/open[0] - 1 = 14.5/10 - 1 = 0.45
        assert!((wf.folds[0].buy_and_hold - 0.45).abs() < 1e-12);
        assert_eq!(wf.folds[0].from, p[0].time);
        assert_eq!(wf.folds[0].to, p[2].time);
        assert_eq!(wf.folds[1].stat.count, 0);
        assert!((wf.folds[2].stat.mean_net - 0.005).abs() < 1e-12);
        // 汇总：空折不计入；positive = fold0,fold2；worst = 0.005
        assert_eq!(wf.positive_folds, 2);
        assert!((wf.worst_mean_net - 0.005).abs() < 1e-12);
    }

    #[test]
    fn fewer_points_than_folds_skips_empty_ranges() {
        let p = bars(2);
        let nets = vec![Some(0.01), Some(0.02)];
        let wf = walk_forward(&nets, &p, 5);
        assert_eq!(wf.folds.len(), 2); // 空索引段折被省略
        assert_eq!(wf.positive_folds, 2);
    }
}
