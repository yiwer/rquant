use crate::report::{Report, SoftReport};
use crate::report::curve::{EquitySeries, Histogram};
use std::fmt::Write;

const W: u32 = 640;
const H: u32 = 240;

fn ny(v: f64, lo: f64, hi: f64, pad: f64) -> f64 {
    let span = if (hi - lo).abs() < 1e-12 { 1.0 } else { hi - lo };
    (H as f64 - pad) - (v - lo) / span * (H as f64 - 2.0 * pad)
}

/// 折线图：points 为 (x_index, y) 序列。
pub fn line_chart(points: &[(f64, f64)], title: &str) -> String {
    let pad = 30.0;
    let mut s = String::new();
    let _ = write!(s, "<svg width=\"{W}\" height=\"{H}\" xmlns=\"http://www.w3.org/2000/svg\">");
    let _ = write!(s, "<text x=\"8\" y=\"16\" font-size=\"13\">{title}</text>");
    if points.is_empty() {
        let _ = write!(s, "<text x=\"8\" y=\"{}\">no data</text></svg>", H / 2);
        return s;
    }
    let mut ymin = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let mut ymax = points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    ymin = ymin.min(0.0);
    ymax = ymax.max(0.0);
    let n = points.len().max(2);
    let px = |i: usize| pad + i as f64 / (n - 1) as f64 * (W as f64 - 2.0 * pad);
    let y0 = ny(0.0, ymin, ymax, pad);
    let _ = write!(s, "<line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"#ccc\"/>", pad, y0, W as f64 - pad, y0);
    let pts: Vec<String> = points.iter().enumerate().map(|(i, p)| format!("{:.1},{:.1}", px(i), ny(p.1, ymin, ymax, pad))).collect();
    let _ = write!(s, "<polyline fill=\"none\" stroke=\"#1565c0\" stroke-width=\"1.5\" points=\"{}\"/>", pts.join(" "));
    let _ = write!(s, "<text x=\"{:.0}\" y=\"{:.0}\" font-size=\"10\">{:.3}</text>", pad, pad, ymax);
    let _ = write!(s, "<text x=\"{:.0}\" y=\"{:.0}\" font-size=\"10\">{:.3}</text>", pad, H as f64 - pad + 8.0, ymin);
    let _ = write!(s, "</svg>");
    s
}

/// 条形图：items 为 (label, value)，正绿负红。
pub fn bar_chart(items: &[(String, f64)], title: &str) -> String {
    let pad = 30.0;
    let mut s = String::new();
    let _ = write!(s, "<svg width=\"{W}\" height=\"{H}\" xmlns=\"http://www.w3.org/2000/svg\">");
    let _ = write!(s, "<text x=\"8\" y=\"16\" font-size=\"13\">{title}</text>");
    if items.is_empty() {
        let _ = write!(s, "<text x=\"8\" y=\"{}\">no data</text></svg>", H / 2);
        return s;
    }
    let maxabs = items.iter().map(|(_, v)| v.abs()).fold(0.0_f64, f64::max).max(1e-12);
    let n = items.len();
    let bw = (W as f64 - 2.0 * pad) / n as f64;
    let y0 = H as f64 / 2.0;
    let _ = write!(s, "<line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"#ccc\"/>", pad, y0, W as f64 - pad, y0);
    for (i, (label, v)) in items.iter().enumerate() {
        let x = pad + i as f64 * bw + bw * 0.15;
        let bh = (v.abs() / maxabs) * (H as f64 / 2.0 - pad);
        let (y, color) = if *v >= 0.0 { (y0 - bh, "#2e7d32") } else { (y0, "#c62828") };
        let _ = write!(s, "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" fill=\"{}\"/>", x, y, bw * 0.7, bh, color);
        let _ = write!(s, "<text x=\"{:.1}\" y=\"{:.0}\" font-size=\"9\" text-anchor=\"middle\">{}</text>", x + bw * 0.35, H as f64 - 6.0, label);
    }
    let _ = write!(s, "</svg>");
    s
}

/// 直方图。
pub fn histogram_svg(hist: &Histogram, title: &str) -> String {
    let pad = 30.0;
    let mut s = String::new();
    let _ = write!(s, "<svg width=\"{W}\" height=\"{H}\" xmlns=\"http://www.w3.org/2000/svg\">");
    let _ = write!(s, "<text x=\"8\" y=\"16\" font-size=\"13\">{title}</text>");
    if hist.bins.is_empty() {
        let _ = write!(s, "<text x=\"8\" y=\"{}\">no data</text></svg>", H / 2);
        return s;
    }
    let maxc = hist.bins.iter().map(|(_, _, c)| *c).max().unwrap_or(1).max(1);
    let n = hist.bins.len();
    let bw = (W as f64 - 2.0 * pad) / n as f64;
    let base = H as f64 - pad;
    for (i, (_, _, c)) in hist.bins.iter().enumerate() {
        let x = pad + i as f64 * bw;
        let bh = (*c as f64 / maxc as f64) * (H as f64 - 2.0 * pad);
        let _ = write!(s, "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" fill=\"#5472d3\"/>", x, base - bh, bw * 0.9, bh);
    }
    let _ = write!(s, "</svg>");
    s
}

/// 拼装自包含 HTML 报告。
pub fn render_html(report: &Report, series: Option<&EquitySeries>) -> String {
    let m = &report.metrics;
    let mut s = String::new();
    let _ = write!(s, "<!doctype html><html><head><meta charset=\"utf-8\"><title>rquant report: {}</title>", report.tree_name);
    let _ = write!(s, "<style>body{{font-family:system-ui,Arial,sans-serif;margin:24px;max-width:720px}}table{{border-collapse:collapse}}td,th{{border:1px solid #ddd;padding:4px 8px;text-align:right}}th{{text-align:left}}.warn{{background:#fff3cd;border:1px solid #ffe08a;padding:8px;border-radius:4px;margin:12px 0}}svg{{border:1px solid #eee;margin:8px 0}}</style></head><body>");
    let _ = write!(s, "<h1>rquant report: {}</h1>", report.tree_name);
    let _ = write!(s, "<table><tr><th>metric</th><th>value</th></tr>");
    let _ = write!(s, "<tr><th>forward_window</th><td>{}</td></tr>", report.forward_window);
    let _ = write!(s, "<tr><th>cost_bps</th><td>{:.1}</td></tr>", report.cost_bps);
    let _ = write!(s, "<tr><th>decisions / scored</th><td>{} / {}</td></tr>", m.total_decisions, m.scored);
    let _ = write!(s, "<tr><th>active n</th><td>{}</td></tr>", m.active.count);
    let _ = write!(s, "<tr><th>active mean_net</th><td>{:.4}</td></tr>", m.active.mean_net);
    let _ = write!(s, "<tr><th>active hit%</th><td>{:.1}</td></tr>", m.active.hit_rate * 100.0);
    let _ = write!(s, "<tr><th>active t</th><td>{:.2}</td></tr>", m.active.t_stat);
    let _ = write!(s, "<tr><th>buy&amp;hold</th><td>{:.4}</td></tr>", m.buy_and_hold);
    let _ = write!(s, "<tr><th>gaps (missing/partial)</th><td>{} / {}</td></tr>", report.gaps.missing_trading_days.len(), report.gaps.partial_days.len());
    let _ = write!(s, "</table>");
    let _ = write!(s, "<div class=\"warn\">{}</div>", m.overlap_warning);
    match series {
        Some(es) => {
            let cum: Vec<(f64, f64)> = es.points.iter().enumerate().map(|(i, p)| (i as f64, p.cum)).collect();
            let _ = write!(s, "{}", line_chart(&cum, "累计前瞻收益（窗口重叠 → 信号质量曲线，非可交易净值）"));
            let _ = write!(s, "{}", histogram_svg(&es.hist, "逐点净收益分布"));
            if es.skipped > 0 {
                let _ = write!(s, "<p>{} 点未计入曲线（越界或时间未匹配）</p>", es.skipped);
            }
        }
        None => {
            let _ = write!(s, "<p>（未提供 --traces/--primary，省略时间序列图）</p>");
        }
    }
    let by_leaf: Vec<(String, f64)> = m.by_leaf.iter().map(|(k, v)| (k.clone(), v.mean_net)).collect();
    let _ = write!(s, "{}", bar_chart(&by_leaf, "各叶子平均净收益"));
    let node: Vec<(String, f64)> = m.node_label_counts.iter().map(|(k, c)| (k.clone(), *c as f64)).collect();
    let _ = write!(s, "{}", bar_chart(&node, "节点命中计数"));
    let _ = write!(s, "</body></html>");
    s
}

/// 软模式报告 HTML：累计期望收益曲线 + expected_net 直方图 + 各叶平均概率条形 + headline。
pub fn render_soft_html(report: &SoftReport, series: &EquitySeries, avg_leaf: &[(String, f64)]) -> String {
    let m = &report.soft;
    let mut s = String::new();
    let _ = write!(s, "<!doctype html><html><head><meta charset=\"utf-8\"><title>rquant soft report: {}</title>", report.tree_name);
    let _ = write!(s, "<style>body{{font-family:system-ui,Arial,sans-serif;margin:24px;max-width:720px}}table{{border-collapse:collapse}}td,th{{border:1px solid #ddd;padding:4px 8px;text-align:right}}th{{text-align:left}}.warn{{background:#fff3cd;border:1px solid #ffe08a;padding:8px;border-radius:4px;margin:12px 0}}svg{{border:1px solid #eee;margin:8px 0}}</style></head><body>");
    let _ = write!(s, "<h1>rquant soft report: {}</h1>", report.tree_name);
    let _ = write!(s, "<table><tr><th>metric</th><th>value</th></tr>");
    let _ = write!(s, "<tr><th>forward_window</th><td>{}</td></tr>", report.forward_window);
    let _ = write!(s, "<tr><th>cost_bps</th><td>{:.1}</td></tr>", report.cost_bps);
    let _ = write!(s, "<tr><th>decisions / scored</th><td>{} / {}</td></tr>", m.total_decisions, m.scored);
    let _ = write!(s, "<tr><th>engaged n</th><td>{}</td></tr>", m.engaged.count);
    let _ = write!(s, "<tr><th>engaged mean_net</th><td>{:.4}</td></tr>", m.engaged.mean_net);
    let _ = write!(s, "<tr><th>engaged hit%</th><td>{:.1}</td></tr>", m.engaged.hit_rate * 100.0);
    let _ = write!(s, "<tr><th>engaged t</th><td>{:.2}</td></tr>", m.engaged.t_stat);
    let _ = write!(s, "<tr><th>buy&amp;hold</th><td>{:.4}</td></tr>", m.buy_and_hold);
    let _ = write!(s, "</table>");
    let _ = write!(s, "<div class=\"warn\">{}</div>", m.overlap_warning);
    let cum: Vec<(f64, f64)> = series.points.iter().enumerate().map(|(i, p)| (i as f64, p.cum)).collect();
    let _ = write!(s, "{}", line_chart(&cum, "累计期望收益（窗口重叠 → 信号质量曲线，非可交易净值）"));
    let _ = write!(s, "{}", histogram_svg(&series.hist, "逐点期望净收益分布"));
    if series.skipped > 0 {
        let _ = write!(s, "<p>{} 点未计入曲线（未计分）</p>", series.skipped);
    }
    let _ = write!(s, "{}", bar_chart(avg_leaf, "各叶平均概率"));
    let _ = write!(s, "</body></html>");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backtest::metrics::compute_metrics;
    use crate::backtest::gaps::GapReport;
    use crate::report::curve::{EquitySeries, Histogram, SeriesPoint};
    use chrono::NaiveDate;

    fn sample_report() -> Report {
        let metrics = compute_metrics(&[], &[]);
        Report { tree_name: "viz".into(), forward_window: 8, cost_bps: 10.0, metrics, gaps: GapReport::default() }
    }

    #[test]
    fn line_chart_has_polyline() {
        let pts = vec![(0.0, 0.0), (1.0, 0.5), (2.0, 0.3)];
        let svg = line_chart(&pts, "t");
        assert!(svg.contains("<svg"));
        assert!(svg.contains("<polyline"));
    }

    #[test]
    fn bar_chart_has_rect() {
        let items = vec![("a".to_string(), 0.2), ("b".to_string(), -0.1)];
        let svg = bar_chart(&items, "t");
        assert!(svg.contains("<rect"));
    }

    #[test]
    fn render_html_is_self_contained_and_deterministic() {
        let report = sample_report();
        let t = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        let es = EquitySeries {
            points: vec![SeriesPoint { t, net: 0.1, cum: 0.1 }],
            hist: Histogram { bins: vec![(0.0, 0.1, 1)] },
            skipped: 0,
        };
        let a = render_html(&report, Some(&es));
        let b = render_html(&report, Some(&es));
        assert_eq!(a, b); // 确定性
        assert!(a.contains("<!doctype html>"));
        assert!(a.contains("viz")); // tree_name
        assert!(a.contains("<svg"));
        assert!(a.contains(&report.metrics.overlap_warning));
    }

    #[test]
    fn render_soft_html_is_self_contained() {
        use crate::report::SoftReport;
        use crate::backtest::soft::SoftMetrics;
        use crate::backtest::metrics::signal_stat;
        use crate::report::curve::{EquitySeries, Histogram, SeriesPoint};
        use chrono::NaiveDate;
        let soft = SoftMetrics {
            total_decisions: 3, scored: 2,
            engaged: signal_stat(&[0.1, 0.2]),
            buy_and_hold: 0.05,
            overlap_warning: "OVLAP".into(),
        };
        let report = SoftReport { tree_name: "softviz".into(), forward_window: 4, cost_bps: 10.0, soft };
        let t = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        let series = EquitySeries {
            points: vec![SeriesPoint { t, net: 0.1, cum: 0.1 }, SeriesPoint { t, net: 0.2, cum: 0.3 }],
            hist: Histogram { bins: vec![(0.0, 0.2, 2)] },
            skipped: 0,
        };
        let avg = vec![("leaf_l".to_string(), 0.7), ("leaf_f".to_string(), 0.3)];
        let a = render_soft_html(&report, &series, &avg);
        let b = render_soft_html(&report, &series, &avg);
        assert_eq!(a, b);
        assert!(a.contains("<!doctype html>"));
        assert!(a.contains("softviz"));
        assert!(a.contains("<polyline"));
        assert!(a.contains("<rect"));
        assert!(a.contains("OVLAP"));
    }
}
