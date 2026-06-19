//! 后验分析器(纯算术,无裁决):端口自 scripts/analyze_{sector,twoleg,deploy}.py。
use std::collections::HashMap;

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
