//! value 部署纸面盘:状态模型 + diff + NAV 滚动(纯算术,纸面只跟踪不下真单)。
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavPoint { pub t: String, pub nav: f64, pub bench_nav: f64 }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthRec { pub as_of: String, pub picks: Vec<String>, pub nav: f64, pub bench_nav: f64, pub n_buy: u32, pub n_sell: u32 }
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeployState {
    pub holdings: Vec<String>,
    pub last_date: Option<String>,
    pub nav: f64,
    pub bench_base: Option<f64>,
    pub nav_history: Vec<NavPoint>,
    pub months: Vec<MonthRec>,
}

/// EW 调仓 diff:买(新进)/卖(移出)/持(都在)。权重=1/N。
pub fn diff(prev: &[String], next: &[String]) -> Vec<crate::dto::DiffRowDto> {
    let pw = if prev.is_empty() { 0.0 } else { 1.0 / prev.len() as f64 };
    let nw = if next.is_empty() { 0.0 } else { 1.0 / next.len() as f64 };
    let pset: BTreeSet<&str> = prev.iter().map(|s| s.as_str()).collect();
    let nset: BTreeSet<&str> = next.iter().map(|s| s.as_str()).collect();
    let mut all: BTreeSet<&str> = BTreeSet::new();
    all.extend(pset.iter().copied());
    all.extend(nset.iter().copied());
    all.into_iter().map(|s| {
        let inp = pset.contains(s);
        let inn = nset.contains(s);
        let action = if inn && !inp { "Buy" } else if inp && !inn { "Sell" } else { "Hold" };
        crate::dto::DiffRowDto {
            symbol: s.to_string(), action: action.to_string(),
            from_w: if inp { pw } else { 0.0 }, to_w: if inn { nw } else { 0.0 },
        }
    }).collect()
}

#[cfg(test)]
mod diff_tests {
    use super::*;
    #[test]
    fn diff_buy_sell_hold() {
        let prev = vec!["A".to_string(), "B".to_string()];
        let next = vec!["B".to_string(), "C".to_string()];
        let d = diff(&prev, &next);
        let get = |s: &str| d.iter().find(|r| r.symbol == s).unwrap();
        assert_eq!(get("A").action, "Sell");
        assert_eq!(get("C").action, "Buy");
        assert_eq!(get("B").action, "Hold");
        assert!((get("C").to_w - 0.5).abs() < 1e-9);
        assert!((get("A").to_w - 0.0).abs() < 1e-9);
    }
}
