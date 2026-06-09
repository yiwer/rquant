/// 简化成本模型：对非空仓收益统一扣往返成本（bps）。
#[derive(Debug, Clone, Copy)]
pub struct CostModel {
    pub round_trip_bps: f64,
}

impl CostModel {
    pub fn apply(&self, gross_return: f64) -> f64 {
        gross_return - self.round_trip_bps / 10000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn applies_round_trip_haircut() {
        let c = CostModel { round_trip_bps: 10.0 }; // 0.10%
        assert_relative_eq!(c.apply(0.05), 0.049, epsilon = 1e-9);
    }
}
