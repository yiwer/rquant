//! 后验分析器(纯算术,无裁决):端口自 scripts/analyze_{sector,twoleg,deploy}.py。
use std::collections::{BTreeMap, HashMap};

pub struct SectorAttrib { pub excess_total: f64, pub alloc_pct: f64, pub select_pct: f64, pub cum: Vec<(String, f64, f64, f64)> }

pub fn sector_attribution(
    rebals: &[(String, Vec<String>)],
    price: &dyn Fn(&str, &str) -> Option<f64>,
    sector_of: &HashMap<String, String>,
    sector_lvl: &dyn Fn(&str, &str) -> Option<f64>,
    bench: &dyn Fn(&str) -> Option<f64>,
) -> SectorAttrib {
    let (mut nav_p, mut nav_a, mut nav_b) = (1.0_f64, 1.0_f64, 1.0_f64);
    let mut cum = Vec::new();
    for i in 0..rebals.len().saturating_sub(1) {
        let (t0, sel) = (&rebals[i].0, &rebals[i].1);
        let t1 = &rebals[i + 1].0;
        if sel.is_empty() { continue; }
        let w = 1.0 / sel.len() as f64;
        let mut rp = 0.0;
        for s in sel {
            if let (Some(p0), Some(p1)) = (price(s, t0), price(s, t1)) {
                if p0 > 0.0 { rp += w * (p1 / p0 - 1.0); }
            }
        }
        let mut sec_w: HashMap<&str, f64> = HashMap::new();
        for s in sel { if let Some(ind) = sector_of.get(s) { *sec_w.entry(ind.as_str()).or_default() += w; } }
        let mut ra = 0.0;
        for (ind, sw) in &sec_w {
            if let (Some(l0), Some(l1)) = (sector_lvl(ind, t0), sector_lvl(ind, t1)) {
                if l0 > 0.0 { ra += sw * (l1 / l0 - 1.0); }
            }
        }
        let rb = match (bench(t0), bench(t1)) { (Some(b0), Some(b1)) if b0 > 0.0 => b1 / b0 - 1.0, _ => 0.0 };
        nav_p *= 1.0 + rp; nav_a *= 1.0 + ra; nav_b *= 1.0 + rb;
        cum.push((t1.clone(), nav_p - 1.0, nav_a - 1.0, nav_b - 1.0));
    }
    let (rp, ra, rb) = (nav_p - 1.0, nav_a - 1.0, nav_b - 1.0);
    let excess = rp - rb;
    let (alloc_pct, select_pct) = if excess.abs() > 1e-12 { ((ra - rb) / excess, (rp - ra) / excess) } else { (0.0, 0.0) };
    SectorAttrib { excess_total: excess, alloc_pct, select_pct, cum }
}

#[cfg(test)]
mod sector_tests {
    use super::*;
    use std::collections::HashMap;
    #[test]
    fn brinson_alloc_select_split() {
        let rebals = vec![("2024-01-02".to_string(), vec!["A".to_string(),"B".to_string()]),
                          ("2024-02-02".to_string(), vec!["A".to_string(),"B".to_string()])];
        let px: HashMap<(&str,&str),f64> = HashMap::from([
            (("A","2024-01-02"),10.0),(("A","2024-02-02"),11.0),
            (("B","2024-01-02"),10.0),(("B","2024-02-02"),12.0)]);
        let price = |s:&str,d:&str| px.get(&(s,d)).copied();
        let sector_of = HashMap::from([("A".to_string(),"甲".to_string()),("B".to_string(),"乙".to_string())]);
        let slv: HashMap<(&str,&str),f64> = HashMap::from([
            (("甲","2024-01-02"),100.0),(("甲","2024-02-02"),110.0),
            (("乙","2024-01-02"),100.0),(("乙","2024-02-02"),110.0)]);
        let sector_lvl = |s:&str,d:&str| slv.get(&(s,d)).copied();
        let bm: HashMap<&str,f64> = HashMap::from([("2024-01-02",100.0),("2024-02-02",105.0)]);
        let bench = |d:&str| bm.get(d).copied();
        let r = sector_attribution(&rebals, &price, &sector_of, &sector_lvl, &bench);
        assert!((r.excess_total - 0.10).abs() < 1e-9);
        assert!((r.alloc_pct - 0.5).abs() < 1e-9);
        assert!((r.select_pct - 0.5).abs() < 1e-9);
    }
}

pub struct TwoLegCell { pub w: f64, pub net_total: f64, pub excess: f64, pub oos_excess: Option<f64>, pub sharpe: f64, pub max_dd: f64 }
pub struct TwoLeg { pub rows: Vec<TwoLegCell>, pub best_w: f64 }

pub fn two_leg(v_nav: &[(String, f64)], g_nav: &[(String, f64)], idx: &BTreeMap<String, f64>, regimes: &[(String, String, String)]) -> TwoLeg {
    let gmap: BTreeMap<&str, f64> = g_nav.iter().map(|(d, v)| (d.as_str(), *v)).collect();
    let aligned: Vec<(String, f64, f64)> = v_nav.iter().filter_map(|(d, vv)| gmap.get(d.as_str()).map(|gv| (d.clone(), *vv, *gv))).collect();
    if aligned.len() < 2 { return TwoLeg { rows: vec![], best_w: 1.0 }; }
    let vseg: Vec<f64> = (0..aligned.len()-1).map(|i| aligned[i+1].1/aligned[i].1 - 1.0).collect();
    let gseg: Vec<f64> = (0..aligned.len()-1).map(|i| aligned[i+1].2/aligned[i].2 - 1.0).collect();
    let days: Vec<String> = aligned.iter().map(|(d,_,_)| d.clone()).collect();
    let oos_lbl = regimes.iter().find(|(l,_,_)| l.contains("OOS")).map(|(l,_,_)| l.clone());
    let win = |nav: &[(String,f64)], d0: &str, d1: &str| -> Option<f64> {
        let sub: Vec<&(String,f64)> = nav.iter().filter(|(d,_)| d0 <= d.as_str() && d.as_str() <= d1).collect();
        if sub.len() < 2 { return None; }
        let sr = sub.last().unwrap().1 / sub[0].1 - 1.0;
        match (crate::index_relative::idx_at(idx, &sub[0].0), crate::index_relative::idx_at(idx, &sub.last().unwrap().0)) {
            (Some(x0), Some(x1)) if x0 != 0.0 => Some(sr - (x1/x0 - 1.0)), _ => None }
    };
    let mut rows = Vec::new();
    for w in [1.0, 0.8, 0.7, 0.6, 0.5, 0.4, 0.3, 0.0] {
        let mut nav = vec![(days[0].clone(), 1.0)]; let mut cur = 1.0;
        let mut rets = Vec::new();
        for i in 0..vseg.len() { let r = w*vseg[i] + (1.0-w)*gseg[i]; rets.push(r); cur *= 1.0+r; nav.push((days[i+1].clone(), cur)); }
        let mean = rets.iter().sum::<f64>()/rets.len() as f64;
        let var = rets.iter().map(|r| (r-mean).powi(2)).sum::<f64>()/(rets.len()-1).max(1) as f64;
        let sd = var.sqrt();
        let sharpe = if sd > 0.0 { mean/sd*(12.0_f64).sqrt() } else { 0.0 };
        let (mut peak, mut dd) = (0.0_f64, 0.0_f64);
        for (_, vv) in &nav { peak = peak.max(*vv); dd = dd.max(1.0 - vv/peak); }
        let total = nav.last().unwrap().1 - 1.0;
        let excess = win(&nav, &days[0], days.last().unwrap()).unwrap_or(total);
        let oos = oos_lbl.as_ref().and_then(|l| regimes.iter().find(|(rl,_,_)| rl==l)).and_then(|(_,f,t)| win(&nav, f, t));
        rows.push(TwoLegCell { w, net_total: total, excess, oos_excess: oos, sharpe, max_dd: dd });
    }
    let shs: Vec<f64> = rows.iter().map(|r| r.sharpe).collect();
    let ooss: Vec<f64> = rows.iter().map(|r| r.oos_excess.unwrap_or(0.0)).collect();
    let nz = |x:f64, lo:f64, hi:f64| if hi>lo {(x-lo)/(hi-lo)} else {0.5};
    let (slo,shi) = (shs.iter().cloned().fold(f64::INFINITY,f64::min), shs.iter().cloned().fold(f64::NEG_INFINITY,f64::max));
    let (olo,ohi) = (ooss.iter().cloned().fold(f64::INFINITY,f64::min), ooss.iter().cloned().fold(f64::NEG_INFINITY,f64::max));
    let best_w = rows.iter().max_by(|a,b| {
        let sa = nz(a.sharpe,slo,shi)+nz(a.oos_excess.unwrap_or(0.0),olo,ohi);
        let sb = nz(b.sharpe,slo,shi)+nz(b.oos_excess.unwrap_or(0.0),olo,ohi);
        sa.partial_cmp(&sb).unwrap() }).map(|c| c.w).unwrap_or(1.0);
    TwoLeg { rows, best_w }
}

#[cfg(test)]
mod twoleg_tests {
    use super::*;
    use std::collections::BTreeMap;
    #[test]
    fn endpoints_recover_each_leg() {
        let v = vec![("2024-01-02".to_string(),1.0),("2024-06-28".to_string(),1.2),("2024-12-31".to_string(),1.5)];
        let g = vec![("2024-01-02".to_string(),1.0),("2024-06-28".to_string(),1.1),("2024-12-31".to_string(),1.3)];
        let idx = BTreeMap::from([("2024-01-02".to_string(),100.0),("2024-12-31".to_string(),110.0)]);
        let r = two_leg(&v, &g, &idx, &[]);
        let w1 = r.rows.iter().find(|c| (c.w-1.0).abs()<1e-9).unwrap();
        let w0 = r.rows.iter().find(|c| c.w.abs()<1e-9).unwrap();
        assert!((w1.net_total - 0.5).abs() < 1e-9);
        assert!((w0.net_total - 0.3).abs() < 1e-9);
        assert!((w1.excess - 0.4).abs() < 1e-9);
    }
}
