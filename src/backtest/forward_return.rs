use crate::backtest::costs::CostModel;
use crate::data::bar::Bar;
use crate::tree::schema::Stance;

#[derive(Debug, Clone, Copy)]
pub struct ForwardResult {
    pub gross: f64,
    pub net: f64,
    pub t1_executable: bool,
}

/// 决策在 bar i（收盘）。入场 = bar[i+1] 开盘；出场 = bar[i+n] 收盘（持有 n 根）。
/// i+n 越界返回 None。flat 收益 0、无成本。T+1：出场日 > 入场日 才算可执行。
pub fn forward_return(
    primary: &[Bar],
    i: usize,
    n: usize,
    stance: Stance,
    costs: &CostModel,
) -> Option<ForwardResult> {
    if n == 0 {
        return None;
    }
    let entry_idx = i + 1;
    let exit_idx = i + n;
    if exit_idx >= primary.len() {
        return None;
    }
    let entry = primary[entry_idx].open;
    let exit = primary[exit_idx].close;
    if entry <= 0.0 {
        return None;
    }
    let dir = match stance {
        Stance::Long => 1.0,
        Stance::Short => -1.0,
        Stance::Flat => 0.0,
    };
    let gross = (exit / entry - 1.0) * dir;
    let net = if dir == 0.0 { 0.0 } else { costs.apply(gross) };
    let t1_executable = primary[exit_idx].time.date() > primary[entry_idx].time.date();
    Some(ForwardResult { gross, net, t1_executable })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backtest::costs::CostModel;
    use crate::data::bar::Bar;
    use crate::tree::schema::Stance;
    use approx::assert_relative_eq;
    use chrono::NaiveDateTime;

    fn bar(t: &str, open: f64, close: f64) -> Bar {
        Bar {
            time: NaiveDateTime::parse_from_str(t, "%Y-%m-%d %H:%M:%S").unwrap(),
            open,
            high: open.max(close),
            low: open.min(close),
            close,
            volume: 1.0,
        }
    }

    fn data() -> Vec<Bar> {
        vec![
            bar("2024-01-02 14:45:00", 9.0, 9.5),
            bar("2024-01-02 15:00:00", 10.0, 10.2),
            bar("2024-01-03 09:45:00", 10.2, 11.0),
        ]
    }

    #[test]
    fn long_return_with_costs_and_t1() {
        let c = CostModel { round_trip_bps: 10.0 };
        let r = forward_return(&data(), 0, 2, Stance::Long, &c).unwrap();
        assert_relative_eq!(r.gross, 0.10, epsilon = 1e-9);
        assert_relative_eq!(r.net, 0.099, epsilon = 1e-9);
        assert!(r.t1_executable);
    }

    #[test]
    fn flat_is_zero_and_out_of_range_is_none() {
        let c = CostModel { round_trip_bps: 10.0 };
        let rf = forward_return(&data(), 0, 2, Stance::Flat, &c).unwrap();
        assert_eq!(rf.net, 0.0);
        assert_eq!(rf.gross, 0.0);
        assert!(forward_return(&data(), 1, 2, Stance::Long, &c).is_none());
    }
}
