use crate::backtest::costs::CostModel;
use crate::backtest::forward_return::forward_return;
use crate::backtest::soft::SoftStepRecord;
use crate::data::bar::Bar;
use crate::engine::trace::Trace;
use chrono::NaiveDateTime;
use std::collections::HashMap;

pub struct SeriesPoint {
    pub t: NaiveDateTime,
    pub net: f64,
    pub cum: f64,
}

pub struct Histogram {
    /// (lo, hi, count) 桶
    pub bins: Vec<(f64, f64, usize)>,
}

pub struct EquitySeries {
    pub points: Vec<SeriesPoint>,
    pub hist: Histogram,
    pub skipped: usize,
}

/// 逐点重算 net：按 trace.t 定位 primary bar，forward_return(stance) → net，累加 cum。
/// 找不到 bar / 越界 → 跳过并计 skipped。
pub fn derive_series(traces: &[Trace], primary: &[Bar], fw: usize, cost: &CostModel) -> EquitySeries {
    let index: HashMap<NaiveDateTime, usize> = primary.iter().enumerate().map(|(i, b)| (b.time, i)).collect();
    let mut points = Vec::new();
    let mut skipped = 0usize;
    let mut cum = 0.0;
    for tr in traces {
        let Some(&i) = index.get(&tr.t) else {
            skipped += 1;
            continue;
        };
        match forward_return(primary, i, fw, tr.stance, cost) {
            Some(fr) => {
                cum += fr.net;
                points.push(SeriesPoint { t: tr.t, net: fr.net, cum });
            }
            None => skipped += 1,
        }
    }
    let hist = histogram(&points);
    EquitySeries { points, hist, skipped }
}

fn histogram(points: &[SeriesPoint]) -> Histogram {
    if points.is_empty() {
        return Histogram { bins: vec![] };
    }
    let nets: Vec<f64> = points.iter().map(|p| p.net).collect();
    let min = nets.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = nets.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    if (max - min).abs() < 1e-12 {
        return Histogram { bins: vec![(min, max, nets.len())] };
    }
    const N: usize = 21;
    let width = (max - min) / N as f64;
    let mut counts = [0usize; N];
    for &x in &nets {
        let mut k = ((x - min) / width) as usize;
        if k >= N {
            k = N - 1;
        }
        counts[k] += 1;
    }
    let bins = (0..N).map(|k| (min + k as f64 * width, min + (k + 1) as f64 * width, counts[k])).collect();
    Histogram { bins }
}

/// 软序列：net = expected_net(Some)，累计 cum，expected_net 直方图；None 计 skipped。
pub fn derive_soft_series(records: &[SoftStepRecord]) -> EquitySeries {
    let mut points = Vec::new();
    let mut skipped = 0usize;
    let mut cum = 0.0;
    for r in records {
        match r.expected_net {
            Some(x) => {
                cum += x;
                points.push(SeriesPoint { t: r.t, net: x, cum });
            }
            None => skipped += 1,
        }
    }
    let hist = histogram(&points);
    EquitySeries { points, hist, skipped }
}

/// 各叶平均质量：每叶 Σ leaf_probs.get(leaf).unwrap_or(0) / records.len()，按叶名排序。空→空。
pub fn avg_leaf_probs(records: &[SoftStepRecord]) -> Vec<(String, f64)> {
    if records.is_empty() {
        return vec![];
    }
    let mut names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for r in records {
        for k in r.leaf_probs.keys() {
            names.insert(k.clone());
        }
    }
    let n = records.len() as f64;
    names
        .into_iter()
        .map(|name| {
            let sum: f64 = records.iter().map(|r| r.leaf_probs.get(&name).copied().unwrap_or(0.0)).sum();
            (name, sum / n)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::trace::Trace;
    use crate::tree::schema::Stance;
    use chrono::NaiveDateTime;

    fn bar(t: &str, open: f64, close: f64) -> Bar {
        Bar { time: NaiveDateTime::parse_from_str(t, "%Y-%m-%d %H:%M:%S").unwrap(),
              open, high: open.max(close), low: open.min(close), close, volume: 1.0 }
    }
    fn trace(t: &str, stance: Stance) -> Trace {
        Trace { t: NaiveDateTime::parse_from_str(t, "%Y-%m-%d %H:%M:%S").unwrap(),
                path: vec![], leaf: "l".into(), stance }
    }

    #[test]
    fn derive_series_cumulates_and_skips() {
        let primary = vec![
            bar("2024-01-02 09:45:00", 9.0, 9.0),
            bar("2024-01-02 10:00:00", 10.0, 10.0),
            bar("2024-01-02 10:15:00", 11.0, 11.0),
        ];
        let cost = CostModel { round_trip_bps: 0.0 };
        // decision at bar 0 (long): entry=bar1.open=10, exit=bar2.close=11 → net=0.1
        // decision at bar 2: out of range (i+1=3) → skipped
        let traces = vec![trace("2024-01-02 09:45:00", Stance::Long), trace("2024-01-02 10:15:00", Stance::Long)];
        let es = derive_series(&traces, &primary, 2, &cost);
        assert_eq!(es.points.len(), 1);
        assert!((es.points[0].net - 0.1).abs() < 1e-9);
        assert!((es.points[0].cum - 0.1).abs() < 1e-9);
        assert_eq!(es.skipped, 1);
    }

    #[test]
    fn derive_series_skips_unmatched_time() {
        let primary = vec![bar("2024-01-02 09:45:00", 9.0, 9.0), bar("2024-01-02 10:00:00", 10.0, 10.0)];
        let cost = CostModel { round_trip_bps: 0.0 };
        let traces = vec![trace("2099-01-01 00:00:00", Stance::Long)];
        let es = derive_series(&traces, &primary, 1, &cost);
        assert_eq!(es.points.len(), 0);
        assert_eq!(es.skipped, 1);
    }

    #[test]
    fn derive_soft_series_cumulates_and_skips() {
        use crate::backtest::soft::SoftStepRecord;
        use std::collections::BTreeMap;
        let t = NaiveDateTime::parse_from_str("2024-01-02 09:45:00", "%Y-%m-%d %H:%M:%S").unwrap();
        let mut lp = BTreeMap::new();
        lp.insert("a".to_string(), 0.6);
        lp.insert("b".to_string(), 0.4);
        let recs = vec![
            SoftStepRecord { t, leaf_probs: lp.clone(), expected_net: Some(0.1) },
            SoftStepRecord { t, leaf_probs: lp.clone(), expected_net: Some(0.2) },
            SoftStepRecord { t, leaf_probs: lp, expected_net: None },
        ];
        let es = derive_soft_series(&recs);
        assert_eq!(es.points.len(), 2);
        assert!((es.points[1].cum - 0.3).abs() < 1e-9);
        assert_eq!(es.skipped, 1);
    }

    #[test]
    fn avg_leaf_probs_means_sum_to_one() {
        use crate::backtest::soft::SoftStepRecord;
        use std::collections::BTreeMap;
        let t = NaiveDateTime::parse_from_str("2024-01-02 09:45:00", "%Y-%m-%d %H:%M:%S").unwrap();
        let mut lp1 = BTreeMap::new(); lp1.insert("a".to_string(), 1.0);
        let mut lp2 = BTreeMap::new(); lp2.insert("a".to_string(), 0.5); lp2.insert("b".to_string(), 0.5);
        let recs = vec![
            SoftStepRecord { t, leaf_probs: lp1, expected_net: Some(0.0) },
            SoftStepRecord { t, leaf_probs: lp2, expected_net: Some(0.0) },
        ];
        let avg = avg_leaf_probs(&recs);
        // a: (1.0+0.5)/2 = 0.75 ; b: (0+0.5)/2 = 0.25 ; sorted by name
        assert_eq!(avg.len(), 2);
        assert_eq!(avg[0].0, "a"); assert!((avg[0].1 - 0.75).abs() < 1e-9);
        assert_eq!(avg[1].0, "b"); assert!((avg[1].1 - 0.25).abs() < 1e-9);
        let sum: f64 = avg.iter().map(|(_, v)| v).sum();
        assert!((sum - 1.0).abs() < 1e-9);
    }
}
