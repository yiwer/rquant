use crate::backtest::costs::CostModel;
use crate::backtest::forward_return::forward_return;
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
}
