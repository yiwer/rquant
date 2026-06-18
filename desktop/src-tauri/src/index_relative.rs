//! 指数相对超额重算:端口自 scripts/iterate.py::to_index_relative。
//! 仅算术(超额=组合累计 − 指数累计),非裁决——可安全在 Rust 重算以支持即时切基准。
use std::collections::BTreeMap;
use std::path::Path;

pub struct IndexRel {
    pub excess_cum: Option<f64>,
    pub curve: Vec<(String, f64)>,          // (day, 组合累计 − 指数累计)
    pub per_regime: Vec<(String, Option<f64>)>, // (label, 窗口超额)
}

pub fn load_index(path: &Path) -> Result<BTreeMap<String, f64>, String> {
    let txt = std::fs::read_to_string(path).map_err(|e| format!("读指数失败 {}: {e}", path.display()))?;
    let mut m = BTreeMap::new();
    for line in txt.lines().skip(1) {
        let mut it = line.split(',');
        let (Some(t), Some(c)) = (it.next(), it.next()) else { continue };
        if let Ok(v) = c.trim().parse::<f64>() {
            m.insert(t.get(..10).unwrap_or(t).to_string(), v);
        }
    }
    if m.is_empty() { return Err("指数数据为空".into()); }
    Ok(m)
}

pub fn idx_at(m: &BTreeMap<String, f64>, day: &str) -> Option<f64> {
    let key = day.to_string();
    m.range(..=key).next_back().map(|(_, v)| *v)
}

pub fn compute(
    holdings: &[(String, f64)],
    regimes: &[(String, String, String)],
    idx: &BTreeMap<String, f64>,
) -> IndexRel {
    let nav: Vec<(String, f64)> = holdings.iter().filter(|(_, v)| *v > 0.0).cloned().collect();
    if nav.len() < 2 {
        return IndexRel { excess_cum: None, curve: vec![], per_regime: vec![] };
    }
    let base_nav = nav[0].1;
    let base_idx = idx_at(idx, &nav[0].0);
    let curve = nav.iter().map(|(d, v)| {
        let strat = v / base_nav - 1.0;
        let ex = match (base_idx, idx_at(idx, d)) {
            (Some(i0), Some(i)) if i0 != 0.0 => strat - (i / i0 - 1.0),
            _ => strat,
        };
        (d.clone(), ex)
    }).collect::<Vec<_>>();
    let excess_cum = curve.last().map(|(_, e)| *e);
    let per_regime = regimes.iter().map(|(label, from, to)| {
        let sub: Vec<&(String, f64)> = nav.iter().filter(|(d, _)| from <= d && d <= to).collect();
        let ex = if sub.len() >= 2 {
            let sr = sub.last().unwrap().1 / sub[0].1 - 1.0;
            match (idx_at(idx, &sub[0].0), idx_at(idx, &sub.last().unwrap().0)) {
                (Some(x0), Some(x1)) if x0 != 0.0 => Some(sr - (x1 / x0 - 1.0)),
                _ => None,
            }
        } else { None };
        (label.clone(), ex)
    }).collect();
    IndexRel { excess_cum, curve, per_regime }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    fn idx() -> BTreeMap<String, f64> {
        BTreeMap::from([
            ("2024-01-02".into(), 100.0),
            ("2024-06-28".into(), 110.0),
            ("2024-12-31".into(), 120.0),
        ])
    }
    #[test]
    fn excess_is_strat_minus_index() {
        let h = vec![("2024-01-02".into(), 1.0), ("2024-12-31".into(), 1.5)];
        let r = compute(&h, &[], &idx());
        assert!((r.excess_cum.unwrap() - 0.30).abs() < 1e-9);
    }
    #[test]
    fn per_regime_excess_windowed() {
        let h = vec![
            ("2024-01-02".into(), 1.0),
            ("2024-06-28".into(), 1.2),
            ("2024-12-31".into(), 1.5),
        ];
        let reg = vec![("H1".to_string(), "2024-01-02".to_string(), "2024-06-28".to_string())];
        let r = compute(&h, &reg, &idx());
        assert_eq!(r.per_regime.len(), 1);
        assert!((r.per_regime[0].1.unwrap() - 0.10).abs() < 1e-9);
    }
    #[test]
    fn idx_at_uses_last_on_or_before() {
        let m = idx();
        assert_eq!(idx_at(&m, "2024-03-01"), Some(100.0));
        assert_eq!(idx_at(&m, "2023-01-01"), None);
    }
}
