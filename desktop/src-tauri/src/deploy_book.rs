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

pub fn ew_return(holdings: &[String], price: &dyn Fn(&str, &str) -> Option<f64>, d0: &str, d1: &str) -> f64 {
    let rets: Vec<f64> = holdings.iter().filter_map(|s| {
        match (price(s, d0), price(s, d1)) { (Some(p0), Some(p1)) if p0 > 0.0 => Some(p1 / p0 - 1.0), _ => None }
    }).collect();
    if rets.is_empty() { 0.0 } else { rets.iter().sum::<f64>() / rets.len() as f64 }
}

pub fn read_state(path: &std::path::Path) -> DeployState {
    std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}
pub fn write_state(path: &std::path::Path, st: &DeployState) -> Result<(), String> {
    std::fs::create_dir_all(path.parent().expect("deploy_book has parent")).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(st).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod nav_tests {
    use super::*;
    use std::collections::HashMap;
    #[test]
    fn ew_return_mean_of_holdings() {
        let px: HashMap<(&str,&str),f64> = HashMap::from([
            (("A","2024-01-31"),10.0),(("A","2024-02-29"),11.0),  // +10%
            (("B","2024-01-31"),10.0),(("B","2024-02-29"),13.0)]);// +30%
        let price = |s:&str,d:&str| px.get(&(s,d)).copied();
        let r = ew_return(&["A".to_string(),"B".to_string()], &price, "2024-01-31", "2024-02-29");
        assert!((r - 0.20).abs() < 1e-9); // (.1+.3)/2
        assert!((ew_return(&[], &price, "2024-01-31", "2024-02-29")).abs() < 1e-9);
    }
}
