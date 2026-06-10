use crate::dsl::eval::{eval_bool, eval_fuzzy, eval_scalar};
use crate::eval::Decision;
use crate::features::context::Context;
use crate::tree::loader::{Branch, Strength};
use crate::tree::schema::Target;
use crate::Result;

pub fn eval_quant(branches: &[Branch], default: &Target, ctx: &Context) -> Result<Decision> {
    for b in branches {
        if eval_bool(&b.when, ctx)? {
            return Ok(Decision {
                goto: b.goto.clone(),
                label: b.label.clone(),
                confidence: 1.0,
                rationale: format!("matched: {}", b.when_src),
            });
        }
    }
    Ok(Decision {
        goto: default.goto.clone(),
        label: default.label.clone(),
        confidence: 0.5,
        rationale: "default (no branch matched)".into(),
    })
}

/// 软模式量化分支分布：按声明顺序对 when-true 分支做"首真泄漏"，
/// 权重 w_i = remaining·clamp01(strength_i)（无 strength → 1.0），残余给 default。Σ weights ≡ 1。
pub fn quant_branch_dist(branches: &[Branch], default: &Target, ctx: &Context) -> Result<Vec<(String, f64)>> {
    let mut out: Vec<(String, f64)> = Vec::new();
    let mut remaining = 1.0_f64;
    for b in branches {
        if eval_bool(&b.when, ctx)? {
            let raw = match &b.strength {
                Some(Strength::Expr(e)) => eval_scalar(e, ctx)?,
                Some(Strength::Auto(scale)) => eval_fuzzy(&b.when, ctx, *scale)?,
                None => 1.0,
            };
            let s = if raw.is_nan() { 0.0 } else { raw.clamp(0.0, 1.0) };
            let w = remaining * s;
            if w > 0.0 {
                out.push((b.goto.clone(), w));
            }
            remaining *= 1.0 - s;
            if remaining <= 1e-12 {
                break;
            }
        }
    }
    if remaining > 1e-12 {
        out.push((default.goto.clone(), remaining));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::bar::{Bar, Window};
    use crate::dsl::parser::parse_str;
    use crate::features::context::Context;
    use crate::tree::loader::{Branch, Strength};
    use crate::tree::schema::Target;
    use chrono::NaiveDate;

    fn ctx(closes: &[f64]) -> Context {
        let base = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        let bars: Vec<Bar> = closes
            .iter()
            .enumerate()
            .map(|(i, &c)| Bar {
                time: base + chrono::Duration::minutes(i as i64 * 15),
                open: c, high: c, low: c, close: c, volume: 1.0,
            })
            .collect();
        let t = bars.last().unwrap().time;
        Context { t, primary: Window { bars: bars.clone() }, context: Window { bars }, news: None, aux: std::collections::BTreeMap::new() }
    }

    fn br(when: &str, goto: &str, label: &str) -> Branch {
        Branch { when: parse_str(when).unwrap(), when_src: when.into(), strength: None, goto: goto.into(), label: label.into() }
    }

    fn br_s(when: &str, goto: &str, label: &str, strength: &str) -> Branch {
        Branch { when: parse_str(when).unwrap(), when_src: when.into(), strength: Some(Strength::Expr(parse_str(strength).unwrap())), goto: goto.into(), label: label.into() }
    }

    #[test]
    fn matches_first_true_branch() {
        let branches = vec![br("close > 100", "a", "hi"), br("close > 1", "b", "mid")];
        let default = Target { goto: "d".into(), label: "none".into() };
        let d = eval_quant(&branches, &default, &ctx(&[1.0, 2.0, 3.0])).unwrap();
        assert_eq!(d.goto, "b");
        assert_eq!(d.label, "mid");
        assert_eq!(d.confidence, 1.0);
    }

    #[test]
    fn falls_back_to_default_when_none_match() {
        let branches = vec![br("close > 100", "a", "hi")];
        let default = Target { goto: "d".into(), label: "none".into() };
        let d = eval_quant(&branches, &default, &ctx(&[1.0, 2.0, 3.0])).unwrap();
        assert_eq!(d.goto, "d");
        assert_eq!(d.confidence, 0.5);
    }

    #[test]
    fn dist_single_no_strength_is_hard() {
        let branches = vec![br("close > 1", "g", "up")];
        let default = Target { goto: "d".into(), label: "none".into() };
        let dist = quant_branch_dist(&branches, &default, &ctx(&[1.0, 2.0, 3.0])).unwrap();
        assert_eq!(dist, vec![("g".to_string(), 1.0)]);
    }

    #[test]
    fn dist_single_strength_leaks_to_default() {
        let branches = vec![br_s("close > 1", "g", "up", "0.7")];
        let default = Target { goto: "d".into(), label: "none".into() };
        let dist = quant_branch_dist(&branches, &default, &ctx(&[1.0, 2.0, 3.0])).unwrap();
        assert_eq!(dist.len(), 2);
        assert_eq!(dist[0].0, "g");
        assert!((dist[0].1 - 0.7).abs() < 1e-9);
        assert_eq!(dist[1].0, "d");
        assert!((dist[1].1 - 0.3).abs() < 1e-9);
    }

    #[test]
    fn dist_two_true_branches_leak() {
        let branches = vec![br_s("close > 1", "a", "x", "0.6"), br_s("close > 1", "b", "y", "0.5")];
        let default = Target { goto: "d".into(), label: "none".into() };
        let dist = quant_branch_dist(&branches, &default, &ctx(&[1.0, 2.0, 3.0])).unwrap();
        assert_eq!(dist.len(), 3);
        assert!((dist[0].1 - 0.6).abs() < 1e-9);
        assert!((dist[1].1 - 0.2).abs() < 1e-9); // 0.4 * 0.5
        assert!((dist[2].1 - 0.2).abs() < 1e-9); // remaining 0.4 * 0.5
        let sum: f64 = dist.iter().map(|(_, w)| w).sum();
        assert!((sum - 1.0).abs() < 1e-9);
    }

    #[test]
    fn dist_no_true_branch_is_all_default() {
        let branches = vec![br("close > 100", "a", "x")];
        let default = Target { goto: "d".into(), label: "none".into() };
        let dist = quant_branch_dist(&branches, &default, &ctx(&[1.0, 2.0, 3.0])).unwrap();
        assert_eq!(dist, vec![("d".to_string(), 1.0)]);
    }

    #[test]
    fn dist_auto_strength_uses_fuzzy_when() {
        // close=10.2 vs 阈值 10：margin 2% / scale 0.02 → 权重 ∈ (0.5, 1)
        let branches = vec![Branch {
            when: parse_str("close > 10").unwrap(),
            when_src: "close > 10".into(),
            strength: Some(Strength::Auto(0.02)),
            goto: "g".into(),
            label: "up".into(),
        }];
        let default = Target { goto: "d".into(), label: "none".into() };
        let dist = quant_branch_dist(&branches, &default, &ctx(&[10.0, 10.1, 10.2])).unwrap();
        assert_eq!(dist[0].0, "g");
        assert!(dist[0].1 > 0.5 && dist[0].1 < 1.0);
        let sum: f64 = dist.iter().map(|(_, w)| w).sum();
        assert!((sum - 1.0).abs() < 1e-9);
    }
}
