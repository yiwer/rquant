use chrono::NaiveDateTime;

/// 一根 K 线。`time` = bar 的收盘时刻（交易所本地 = Asia/Shanghai 墙钟，naive）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bar {
    pub time: NaiveDateTime,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

/// 一个时间窗口（按时间升序，最后一根 = 决策时刻最近的已收盘 bar）。
#[derive(Debug, Clone)]
pub struct Window {
    pub bars: Vec<Bar>,
}

impl Window {
    pub fn closes(&self) -> Vec<f64> { self.bars.iter().map(|b| b.close).collect() }
    pub fn opens(&self) -> Vec<f64> { self.bars.iter().map(|b| b.open).collect() }
    pub fn highs(&self) -> Vec<f64> { self.bars.iter().map(|b| b.high).collect() }
    pub fn lows(&self) -> Vec<f64> { self.bars.iter().map(|b| b.low).collect() }
    pub fn volumes(&self) -> Vec<f64> { self.bars.iter().map(|b| b.volume).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn dt(h: u32, m: u32) -> chrono::NaiveDateTime {
        NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn window_accessors_extract_fields() {
        let bars = vec![
            Bar { time: dt(9, 45), open: 1.0, high: 2.0, low: 0.5, close: 1.5, volume: 100.0 },
            Bar { time: dt(10, 0), open: 1.5, high: 2.5, low: 1.0, close: 2.0, volume: 200.0 },
        ];
        let w = Window { bars };
        assert_eq!(w.closes(), vec![1.5, 2.0]);
        assert_eq!(w.opens(), vec![1.0, 1.5]);
        assert_eq!(w.highs(), vec![2.0, 2.5]);
        assert_eq!(w.lows(), vec![0.5, 1.0]);
        assert_eq!(w.volumes(), vec![100.0, 200.0]);
    }
}
