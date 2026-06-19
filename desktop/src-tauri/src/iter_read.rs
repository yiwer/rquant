//! 迭代 ledger 只读解析:JSONL 轮次、md 队列段、门槛展示映射。
//! gates_from 仅把 Python judge 已下的 flags/metrics 映射成展示行,绝不在 Rust 重新裁决。
use crate::dto_iter::{IterQueueDto, LedgerRoundDto, RoundCardDto, RoundGateDto, Tier2CellDto};

pub fn parse_ledger(jsonl: &str) -> Vec<LedgerRoundDto> {
    jsonl.lines().filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<LedgerRoundDto>(l).ok())
        .collect()
}

pub fn parse_queue(md: &str) -> IterQueueDto {
    fn items_after(md: &str, marker: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut in_sec = false;
        for line in md.lines() {
            if line.starts_with("## ") {
                in_sec = line.contains(marker);
                continue;
            }
            if in_sec {
                let trimmed = line.trim_start();
                if trimmed.starts_with('-') || trimmed.starts_with('•') || trimmed.starts_with('*') {
                    let t = trimmed.trim_start_matches(['-', '•', '*', ' ']).trim();
                    if !t.is_empty() {
                        out.push(t.to_string());
                    }
                }
            }
        }
        out
    }
    IterQueueDto { falsified: items_after(md, "已证伪角度"), queue: items_after(md, "待试角度") }
}

pub fn gates_from(r: &LedgerRoundDto) -> Vec<RoundGateDto> {
    let be_flag = r.flags.iter().any(|x| x.starts_with("break-even<"));
    vec![
        RoundGateDto { name: "毛超额>0".into(), pass: !r.flags.iter().any(|x| x == "gross-excess<=0"), value: r.gross_ex, threshold: Some(0.0), note: "源头有超额".into() },
        RoundGateDto { name: "净·样本外超额>0".into(), pass: !r.flags.iter().any(|x| x == "net-OOS<=0"), value: r.net_oos_ex, threshold: Some(0.0), note: "金标准".into() },
        RoundGateDto { name: "净夏普>0".into(), pass: !r.flags.iter().any(|x| x == "net-sharpe<=0"), value: r.net_sharpe, threshold: Some(0.0), note: String::new() },
        RoundGateDto { name: "盈亏平衡≥40基点".into(), pass: !be_flag, value: r.break_even, threshold: Some(40.0), note: "≥2×成本".into() },
        RoundGateDto { name: "无符号翻转".into(), pass: !r.flags.iter().any(|x| x == "sign-flip"), value: None, threshold: None, note: "敏感性扫描·二阶".into() },
    ]
}

pub fn round_card(r: &LedgerRoundDto, tier2: Vec<Tier2CellDto>, config_path: String) -> RoundCardDto {
    RoundCardDto {
        round: r.round, label: r.label.clone(), benchmark: r.benchmark.clone(),
        rebalance: r.rebalance, verdict: r.verdict.clone(),
        gates: gates_from(r), tier2, flags: r.flags.clone(), config_path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_ledger_skips_bad_lines() {
        let j = "{\"round\":4,\"label\":\"value_pb\",\"verdict\":\"PASS\",\"flags\":[],\"net_oos_ex\":0.64,\"net_sharpe\":1.13,\"gross_ex\":3.1,\"break_even\":164.0}\nnot-json\n{\"round\":1,\"label\":\"corr\",\"verdict\":\"FALSIFIED\",\"flags\":[\"gross-excess<=0\"]}";
        let v = parse_ledger(j);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].round, 4);
        assert_eq!(v[1].verdict, "FALSIFIED");
    }
    #[test]
    fn gates_map_flags_to_pass_fail_without_rejudging() {
        let r = LedgerRoundDto { round:1, label:"x".into(), axis:String::new(), note:String::new(),
            benchmark:"csi300".into(), rebalance:1, verdict:"FALSIFIED".into(),
            flags: vec!["gross-excess<=0".into(), "break-even<40bps".into()],
            gross_ex:Some(-0.1), net_ex:None, net_oos_ex:Some(0.2), net_train_ex:None, net_sharpe:Some(0.5), break_even:Some(10.0) };
        let g = gates_from(&r);
        assert!(!g.iter().find(|x| x.name=="毛超额>0").unwrap().pass);
        assert!(g.iter().find(|x| x.name=="净·样本外超额>0").unwrap().pass);
        assert!(g.iter().find(|x| x.name=="无符号翻转").unwrap().pass);
    }
    #[test]
    fn parse_queue_extracts_two_sections() {
        let md = "## 已证伪角度（勿重试）\n- 动量\n- 反转\n\n## 待试角度（候选队列）\n- 股息率\n- 低波\n## 其他\n- 噪声\n";
        let q = parse_queue(md);
        assert_eq!(q.falsified, vec!["动量", "反转"]);
        assert_eq!(q.queue, vec!["股息率", "低波"]);
    }
}
