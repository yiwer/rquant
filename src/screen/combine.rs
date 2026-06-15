//! 纯合并逻辑：每股的「逐树标量」→ 优质分 + 形态投票标签 + 综合分（双输出）。
//! 真横截面（跨标的排名/选股）不在这里——由编排器用 portfolio::select_top 做。

use std::collections::BTreeMap;

/// 合并参数。
#[derive(Debug, Clone, Copy)]
pub struct MergeParams {
    pub theta_fire: f64,
    pub vote_frac: f64,
    pub q_floor: f64,
}

/// 单股合并输出。
#[derive(Debug, Clone, PartialEq)]
pub struct CombineOutput {
    pub quality_score: f64,
    pub speculative_score: f64,
    pub combined_score: f64,
    /// 命中（投票通过）的形态标签，按标签名升序（BTreeMap 保序）。
    pub tags: Vec<String>,
    /// 命中形态 -> 强度（用于回测归因）。
    pub setup_strength: BTreeMap<String, f64>,
}

/// 有限值均值；无有限值 → 0。
fn mean_finite(xs: &[f64]) -> f64 {
    let v: Vec<f64> = xs.iter().copied().filter(|x| x.is_finite()).collect();
    if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 }
}

/// 单形态投票：命中当 count(s >= theta_fire) >= ceil(n*vote_frac)（下限 1）；
/// 强度 = 命中树得分均值（未命中 → (false, 0)）。
pub fn setup_vote(scores: &[f64], theta_fire: f64, vote_frac: f64) -> (bool, f64) {
    let finite: Vec<f64> = scores.iter().copied().filter(|x| x.is_finite()).collect();
    let n = finite.len();
    if n == 0 {
        return (false, 0.0);
    }
    let need = ((n as f64 * vote_frac).ceil() as usize).max(1);
    let firing: Vec<f64> = finite.into_iter().filter(|s| *s >= theta_fire).collect();
    if firing.len() >= need {
        let strength = firing.iter().sum::<f64>() / firing.len() as f64;
        (true, strength)
    } else {
        (false, 0.0)
    }
}

/// 合并：优质分 = 优质树得分均值；形态 = 投票；投机分 = 命中形态最大强度；
/// 综合分 = 优质×投机，但不合格（无标签 或 优质<q_floor）→ 0。
pub fn combine(
    quality: &[f64],
    setups: &BTreeMap<String, Vec<f64>>,
    p: &MergeParams,
) -> CombineOutput {
    let q = mean_finite(quality);
    let mut tags = Vec::new();
    let mut setup_strength: BTreeMap<String, f64> = BTreeMap::new();
    for (tag, scores) in setups {
        let (fired, strength) = setup_vote(scores, p.theta_fire, p.vote_frac);
        if fired {
            tags.push(tag.clone());
            setup_strength.insert(tag.clone(), strength);
        }
    }
    let spec = setup_strength.values().copied().fold(0.0_f64, f64::max);
    let eligible = !tags.is_empty() && q >= p.q_floor;
    let combined = if eligible { q * spec } else { 0.0 };
    CombineOutput {
        quality_score: q,
        speculative_score: spec,
        combined_score: combined,
        tags,
        setup_strength,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p() -> MergeParams { MergeParams { theta_fire: 0.5, vote_frac: 0.5, q_floor: 0.5 } }

    #[test]
    fn vote_single_tree_fires_when_above_theta() {
        assert_eq!(setup_vote(&[0.7], 0.5, 0.5), (true, 0.7));
        assert_eq!(setup_vote(&[0.3], 0.5, 0.5), (false, 0.0));
    }

    #[test]
    fn vote_majority_of_three() {
        // need = ceil(3*0.5) = 2
        assert!(setup_vote(&[0.6, 0.8, 0.1], 0.5, 0.5).0);  // 2 fire
        assert!(!setup_vote(&[0.6, 0.1, 0.1], 0.5, 0.5).0); // 1 fires < 2
        let (fired, strength) = setup_vote(&[0.6, 0.8, 0.1], 0.5, 0.5);
        assert!(fired);
        assert!((strength - 0.7).abs() < 1e-12); // mean of firing {0.6,0.8}
    }

    #[test]
    fn vote_empty_or_nan() {
        assert_eq!(setup_vote(&[], 0.5, 0.5), (false, 0.0));
        assert_eq!(setup_vote(&[f64::NAN], 0.5, 0.5), (false, 0.0));
    }

    #[test]
    fn combine_quality_is_mean() {
        let setups = BTreeMap::new();
        let out = combine(&[1.0, 0.5], &setups, &p());
        assert!((out.quality_score - 0.75).abs() < 1e-12);
    }

    #[test]
    fn combine_tags_and_combined_score() {
        let mut setups = BTreeMap::new();
        setups.insert("动量延续".to_string(), vec![0.8]);
        setups.insert("超跌反弹".to_string(), vec![0.2]); // below theta → not fired
        let out = combine(&[0.9], &setups, &p());
        assert_eq!(out.tags, vec!["动量延续".to_string()]);
        assert!((out.speculative_score - 0.8).abs() < 1e-12);
        assert!((out.combined_score - 0.9 * 0.8).abs() < 1e-12);
    }

    #[test]
    fn combine_ineligible_when_no_tags() {
        let setups = BTreeMap::new();
        let out = combine(&[1.0], &setups, &p());
        assert!(out.tags.is_empty());
        assert_eq!(out.combined_score, 0.0);
    }

    #[test]
    fn combine_ineligible_when_quality_below_floor() {
        let mut setups = BTreeMap::new();
        setups.insert("动量延续".to_string(), vec![0.9]);
        let out = combine(&[0.3], &setups, &p()); // quality 0.3 < q_floor 0.5
        assert_eq!(out.tags, vec!["动量延续".to_string()]); // tag still reported
        assert_eq!(out.combined_score, 0.0);               // but not eligible
    }

    #[test]
    fn combine_speculative_is_max_over_setups() {
        let mut setups = BTreeMap::new();
        setups.insert("动量延续".to_string(), vec![0.6]);
        setups.insert("突破临界".to_string(), vec![0.9]);
        let out = combine(&[1.0], &setups, &p());
        assert_eq!(out.tags.len(), 2);
        assert!((out.speculative_score - 0.9).abs() < 1e-12);
    }
}
