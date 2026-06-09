use crate::dsl::eval::eval_bool;
use crate::eval::Decision;
use crate::features::context::Context;
use crate::tree::loader::Branch;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::bar::{Bar, Window};
    use crate::dsl::parser::parse_str;
    use crate::features::context::Context;
    use crate::tree::loader::Branch;
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
        Context { t, primary: Window { bars: bars.clone() }, context: Window { bars }, news: None }
    }

    fn br(when: &str, goto: &str, label: &str) -> Branch {
        Branch { when: parse_str(when).unwrap(), when_src: when.into(), strength: None, goto: goto.into(), label: label.into() }
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
}
