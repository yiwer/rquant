use crate::report::{Report, SoftReport};
use crate::report::curve::{EquitySeries, Histogram, StackSeries};
use crate::backtest::sim::{SimReport, SimStepRecord};
use crate::backtest::portfolio::PortfolioReport;
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

const PALETTE: [&str; 6] = ["#1565c0", "#2e7d32", "#c62828", "#f9a825", "#6a1b9a", "#00838f"];

/// 多线折线图（≤PALETTE 数线），PALETTE 着色 + 顶部图例；y 域 = 全体点 min/max。
pub fn multi_line_chart(series: &[(String, Vec<(f64, f64)>)], title: &str) -> String {
    let pad = 30.0;
    let mut s = String::new();
    let _ = write!(s, "<svg width=\"{W}\" height=\"{H}\" xmlns=\"http://www.w3.org/2000/svg\">");
    let _ = write!(s, "<text x=\"8\" y=\"16\" font-size=\"13\">{title}</text>");
    let all: Vec<f64> = series.iter().flat_map(|(_, pts)| pts.iter().map(|p| p.1)).collect();
    if all.is_empty() {
        let _ = write!(s, "<text x=\"8\" y=\"{}\">no data</text></svg>", H / 2);
        return s;
    }
    let ymin = all.iter().copied().fold(f64::INFINITY, f64::min);
    let ymax = all.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let n = series.iter().map(|(_, p)| p.len()).max().unwrap_or(2).max(2);
    let px = |i: usize| pad + i as f64 / (n - 1) as f64 * (W as f64 - 2.0 * pad);
    for (k, (name, pts)) in series.iter().enumerate() {
        let color = PALETTE[k % PALETTE.len()];
        let path: Vec<String> = pts.iter().enumerate().map(|(i, p)| format!("{:.1},{:.1}", px(i), ny(p.1, ymin, ymax, pad))).collect();
        let _ = write!(s, "<polyline fill=\"none\" stroke=\"{}\" stroke-width=\"1.5\" points=\"{}\"/>", color, path.join(" "));
        let lx = pad + k as f64 * 100.0;
        let _ = write!(s, "<rect x=\"{:.0}\" y=\"22\" width=\"10\" height=\"10\" fill=\"{}\"/>", lx, color);
        let _ = write!(s, "<text x=\"{:.0}\" y=\"31\" font-size=\"10\">{}</text>", lx + 14.0, name);
    }
    let _ = write!(s, "</svg>");
    s
}

/// 叶子概率堆叠面积图：y 域固定 \[0,1\]，每层 polygon（上=本层累计、下=前层累计），图例置顶。
pub fn stacked_area_chart(stack: &StackSeries, title: &str) -> String {
    let pad = 30.0;
    let mut s = String::new();
    let _ = write!(s, "<svg width=\"{W}\" height=\"{H}\" xmlns=\"http://www.w3.org/2000/svg\">");
    let _ = write!(s, "<text x=\"8\" y=\"16\" font-size=\"13\">{title}</text>");
    if stack.rows.is_empty() || stack.names.is_empty() {
        let _ = write!(s, "<text x=\"8\" y=\"{}\">no data</text></svg>", H / 2);
        return s;
    }
    let n = stack.rows.len();
    let px = |i: usize| pad + i as f64 / (n.max(2) - 1) as f64 * (W as f64 - 2.0 * pad);
    let py = |v: f64| ny(v, 0.0, 1.0, pad);
    for (k, name) in stack.names.iter().enumerate() {
        let color = PALETTE[k % PALETTE.len()];
        let mut pts = String::new();
        for (i, row) in stack.rows.iter().enumerate() {
            let _ = write!(pts, "{:.1},{:.1} ", px(i), py(row[k]));
        }
        for (i, row) in stack.rows.iter().enumerate().rev() {
            let lower = if k == 0 { 0.0 } else { row[k - 1] };
            let _ = write!(pts, "{:.1},{:.1} ", px(i), py(lower));
        }
        let _ = write!(s, "<polygon points=\"{}\" fill=\"{}\" fill-opacity=\"0.8\"/>", pts.trim_end(), color);
        // 图例
        let lx = pad + k as f64 * 100.0;
        let _ = write!(s, "<rect x=\"{:.0}\" y=\"22\" width=\"10\" height=\"10\" fill=\"{}\"/>", lx, color);
        let _ = write!(s, "<text x=\"{:.0}\" y=\"31\" font-size=\"10\">{}</text>", lx + 14.0, name);
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
    let _ = write!(s, "<tr><th>active t</th><td>{}</td></tr>", m.active.t_stat.map_or("—".to_string(), |v| format!("{:.2}", v)));
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
    if let Some(wf) = &report.walk_forward {
        let items: Vec<(String, f64)> = wf
            .folds
            .iter()
            .enumerate()
            .map(|(i, f)| (format!("f{}", i + 1), f.stat.mean_net))
            .collect();
        let _ = write!(s, "{}", bar_chart(&items, "walk-forward 各折 mean_net"));
        let _ = write!(s, "<p>walk-forward: positive {}/{}, worst mean {:.4}</p>", wf.positive_folds, wf.folds.len(), wf.worst_mean_net);
    }
    let _ = write!(s, "</body></html>");
    s
}

/// 软模式报告 HTML：累计期望收益曲线 + expected_net 直方图 + 各叶平均概率条形 + headline。
pub fn render_soft_html(report: &SoftReport, series: &EquitySeries, avg_leaf: &[(String, f64)], stack: Option<&StackSeries>) -> String {
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
    let _ = write!(s, "<tr><th>engaged t</th><td>{}</td></tr>", m.engaged.t_stat.map_or("—".to_string(), |v| format!("{:.2}", v)));
    let _ = write!(s, "<tr><th>position n</th><td>{}</td></tr>", m.position.count);
    let _ = write!(s, "<tr><th>position mean_net</th><td>{:.4}</td></tr>", m.position.mean_net);
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
    if let Some(st) = stack {
        let _ = write!(s, "{}", stacked_area_chart(st, "叶子概率随时间（堆叠，Σ=1）"));
    }
    if let Some(wf) = &report.walk_forward {
        let items: Vec<(String, f64)> = wf
            .folds
            .iter()
            .enumerate()
            .map(|(i, f)| (format!("f{}", i + 1), f.stat.mean_net))
            .collect();
        let _ = write!(s, "{}", bar_chart(&items, "walk-forward 各折 mean_net"));
        let _ = write!(s, "<p>walk-forward: positive {}/{}, worst mean {:.4}</p>", wf.positive_folds, wf.folds.len(), wf.worst_mean_net);
    }
    let _ = write!(s, "</body></html>");
    s
}

/// 自包含 HTML 报告：单标的 sim 回测（净值曲线、仓位轨迹、回合直方图、回合表）。
pub fn render_sim_html(report: &SimReport, steps: Option<&[SimStepRecord]>) -> String {
    let mut s = String::new();
    let _ = write!(s, "<!doctype html><html><head><meta charset=\"utf-8\"><title>rquant sim report: {}</title>", report.tree_name);
    let _ = write!(s, "<style>body{{font-family:system-ui,Arial,sans-serif;margin:24px;max-width:720px}}table{{border-collapse:collapse}}td,th{{border:1px solid #ddd;padding:4px 8px;text-align:right}}th{{text-align:left}}.warn{{background:#fff3cd;border:1px solid #ffe08a;padding:8px;border-radius:4px;margin:12px 0}}svg{{border:1px solid #eee;margin:8px 0}}</style></head><body>");
    let _ = write!(s, "<h1>rquant sim report: {}</h1>", report.tree_name);
    // headline 表
    let _ = write!(s, "<table><tr><th>metric</th><th>value</th></tr>");
    let _ = write!(s, "<tr><th>total_return</th><td>{:.4}</td></tr>", report.total_return);
    let _ = write!(s, "<tr><th>max_drawdown</th><td>{:.4}</td></tr>", report.max_drawdown);
    let _ = write!(s, "<tr><th>n_round_trips</th><td>{}</td></tr>", report.n_round_trips);
    let _ = write!(s, "<tr><th>win_rate</th><td>{:.1}%</td></tr>", report.win_rate * 100.0);
    let _ = write!(s, "<tr><th>avg_hold_bars</th><td>{:.1}</td></tr>", report.avg_hold_bars);
    let _ = write!(s, "<tr><th>turnover</th><td>{:.4}</td></tr>", report.turnover);
    let _ = write!(s, "<tr><th>buy&amp;hold</th><td>{:.4}</td></tr>", report.buy_and_hold);
    // 风险指标行
    let opt_fmt = |v: Option<f64>| v.map_or("—".to_string(), |x| format!("{:.2}", x));
    let var_fmt = |v: f64| format!("{:+.4}", v);
    if let Some(r) = &report.risk {
        let _ = write!(s, "<tr><th>年化收益</th><td>{}</td></tr>", opt_fmt(r.ann_return));
        let _ = write!(s, "<tr><th>年化波动</th><td>{}</td></tr>", opt_fmt(r.ann_vol));
        let _ = write!(s, "<tr><th>Sharpe</th><td>{}</td></tr>", opt_fmt(r.sharpe));
        let _ = write!(s, "<tr><th>Sortino</th><td>{}</td></tr>", opt_fmt(r.sortino));
        let _ = write!(s, "<tr><th>Calmar</th><td>{}</td></tr>", opt_fmt(r.calmar));
        let _ = write!(s, "<tr><th>VaR95</th><td>{}</td></tr>", var_fmt(r.var95));
        let _ = write!(s, "<tr><th>CVaR95</th><td>{}</td></tr>", var_fmt(r.cvar95));
    } else {
        let _ = write!(s, "<tr><th>年化收益</th><td>—</td></tr>");
        let _ = write!(s, "<tr><th>年化波动</th><td>—</td></tr>");
        let _ = write!(s, "<tr><th>Sharpe</th><td>—</td></tr>");
        let _ = write!(s, "<tr><th>Sortino</th><td>—</td></tr>");
        let _ = write!(s, "<tr><th>Calmar</th><td>—</td></tr>");
        let _ = write!(s, "<tr><th>VaR95</th><td>—</td></tr>");
        let _ = write!(s, "<tr><th>CVaR95</th><td>—</td></tr>");
    }
    let _ = write!(s, "</table>");
    // 净值/仓位曲线 or 占位
    match steps {
        Some(steps_slice) => {
            let nav_pts: Vec<(f64, f64)> = steps_slice.iter().enumerate().map(|(i, r)| (i as f64, r.nav)).collect();
            let pos_pts: Vec<(f64, f64)> = steps_slice.iter().enumerate().map(|(i, r)| (i as f64, r.pos)).collect();
            let _ = write!(s, "{}", line_chart(&nav_pts, "净值曲线（顺序权益）"));
            let _ = write!(s, "{}", line_chart(&pos_pts, "仓位轨迹"));
        }
        None => {
            let _ = write!(s, "<p>（未提供 --traces，省略净值/仓位曲线）</p>");
        }
    }
    // 回合收益分布直方图
    let trip_returns: Vec<f64> = report.trades.iter().map(|t| t.trip_return).collect();
    let hist = crate::report::curve::histogram_of(&trip_returns);
    let _ = write!(s, "{}", histogram_svg(&hist, "回合收益分布"));
    // 回合表
    let _ = write!(s, "<table><tr><th>entry_t</th><th>exit_t</th><th>entry_px</th><th>exit_px</th><th>trip_return</th><th>bars_held</th><th>reason</th></tr>");
    for t in report.trades.iter().take(50) {
        let _ = write!(s,
            "<tr><td>{}</td><td>{}</td><td>{:.4}</td><td>{:.4}</td><td>{:.4}</td><td>{}</td><td>{}</td></tr>",
            t.entry_t, t.exit_t, t.entry_px, t.exit_px, t.trip_return, t.bars_held, t.reason
        );
    }
    let _ = write!(s, "</table>");
    if report.trades.len() > 50 {
        let _ = write!(s, "<p>（共 {} 回合，仅显示前 50）</p>", report.trades.len());
    }
    let _ = write!(s, "</body></html>");
    s
}

/// 自包含 HTML 报告：横截面组合（组合 vs 基准双线净值、选中频率、持仓表）。
pub fn render_portfolio_html(report: &PortfolioReport) -> String {
    use std::collections::BTreeMap;
    let mut s = String::new();
    let _ = write!(s, "<!doctype html><html><head><meta charset=\"utf-8\"><title>rquant portfolio report: {}</title>", report.tree_name);
    let _ = write!(s, "<style>body{{font-family:system-ui,Arial,sans-serif;margin:24px;max-width:720px}}table{{border-collapse:collapse}}td,th{{border:1px solid #ddd;padding:4px 8px;text-align:right}}th{{text-align:left}}.warn{{background:#fff3cd;border:1px solid #ffe08a;padding:8px;border-radius:4px;margin:12px 0}}svg{{border:1px solid #eee;margin:8px 0}}</style></head><body>");
    let _ = write!(s, "<h1>rquant portfolio report: {}</h1>", report.tree_name);
    // headline 表
    let _ = write!(s, "<table><tr><th>metric</th><th>value</th></tr>");
    let _ = write!(s, "<tr><th>total_return</th><td>{:.4}</td></tr>", report.total_return);
    let _ = write!(s, "<tr><th>benchmark_return</th><td>{:.4}</td></tr>", report.benchmark_return);
    let _ = write!(s, "<tr><th>超额收益</th><td>{:.4}</td></tr>", report.total_return - report.benchmark_return);
    let _ = write!(s, "<tr><th>max_drawdown</th><td>{:.4}</td></tr>", report.max_drawdown);
    let _ = write!(s, "<tr><th>turnover</th><td>{:.4}</td></tr>", report.turnover);
    let _ = write!(s, "<tr><th>n_rebalances</th><td>{}</td></tr>", report.n_rebalances);
    let _ = write!(s, "<tr><th>avg_members</th><td>{:.2}</td></tr>", report.avg_members);
    // 风险指标行
    let opt_fmt = |v: Option<f64>| v.map_or("—".to_string(), |x| format!("{:.2}", x));
    let var_fmt = |v: f64| format!("{:+.4}", v);
    if let Some(r) = &report.risk {
        let _ = write!(s, "<tr><th>年化收益</th><td>{}</td></tr>", opt_fmt(r.ann_return));
        let _ = write!(s, "<tr><th>年化波动</th><td>{}</td></tr>", opt_fmt(r.ann_vol));
        let _ = write!(s, "<tr><th>Sharpe</th><td>{}</td></tr>", opt_fmt(r.sharpe));
        let _ = write!(s, "<tr><th>Sortino</th><td>{}</td></tr>", opt_fmt(r.sortino));
        let _ = write!(s, "<tr><th>Calmar</th><td>{}</td></tr>", opt_fmt(r.calmar));
        let _ = write!(s, "<tr><th>VaR95</th><td>{}</td></tr>", var_fmt(r.var95));
        let _ = write!(s, "<tr><th>CVaR95</th><td>{}</td></tr>", var_fmt(r.cvar95));
    } else {
        let _ = write!(s, "<tr><th>年化收益</th><td>—</td></tr>");
        let _ = write!(s, "<tr><th>年化波动</th><td>—</td></tr>");
        let _ = write!(s, "<tr><th>Sharpe</th><td>—</td></tr>");
        let _ = write!(s, "<tr><th>Sortino</th><td>—</td></tr>");
        let _ = write!(s, "<tr><th>Calmar</th><td>—</td></tr>");
        let _ = write!(s, "<tr><th>VaR95</th><td>—</td></tr>");
        let _ = write!(s, "<tr><th>CVaR95</th><td>—</td></tr>");
    }
    let _ = write!(s, "</table>");
    // 组合 vs 基准双线
    let nav_series: Vec<(f64, f64)> = report.holdings.iter().enumerate().map(|(i, h)| (i as f64, h.nav)).collect();
    let bnav_series: Vec<(f64, f64)> = report.holdings.iter().enumerate().map(|(i, h)| (i as f64, h.benchmark_nav)).collect();
    let series = vec![
        ("组合".to_string(), nav_series),
        ("基准".to_string(), bnav_series),
    ];
    let _ = write!(s, "{}", multi_line_chart(&series, "组合 vs 基准净值（x=调仓序）"));
    // 选中频率
    let n_rb = report.n_rebalances.max(1);
    let mut freq_map: BTreeMap<String, usize> = BTreeMap::new();
    for h in &report.holdings {
        for (sym, _) in &h.selected {
            *freq_map.entry(sym.clone()).or_insert(0) += 1;
        }
    }
    let freq_items: Vec<(String, f64)> = freq_map.iter().map(|(k, v)| (k.clone(), *v as f64 / n_rb as f64)).collect();
    let _ = write!(s, "{}", bar_chart(&freq_items, "选中频率"));
    // 持仓表
    let _ = write!(s, "<table><tr><th>t</th><th>selected</th></tr>");
    for h in report.holdings.iter().take(50) {
        let sel_str: Vec<String> = h.selected.iter().map(|(sym, score)| format!("{}({:.3})", sym, score)).collect();
        let _ = write!(s, "<tr><td>{}</td><td>{}</td></tr>", h.t, sel_str.join(", "));
    }
    let _ = write!(s, "</table>");
    if report.holdings.len() > 50 {
        let _ = write!(s, "<p>（共 {} 调仓记录，仅显示前 50）</p>", report.holdings.len());
    }
    let _ = write!(s, "</body></html>");
    s
}

/// 自包含 HTML 报告：因子检验工作台（IC 衰减、分层年化、spread 净值、相关性矩阵）。
pub fn render_factor_html(report: &crate::factor::FactorReport) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let opt_fmt = |v: Option<f64>| v.map_or("—".to_string(), |x| format!("{:.4}", x));
    let opt_fmt2 = |v: Option<f64>| v.map_or("—".to_string(), |x| format!("{:.2}", x));

    let _ = write!(s, "<!doctype html><html><head><meta charset=\"utf-8\"><title>rquant factor report</title>");
    let _ = write!(s, "<style>body{{font-family:system-ui,Arial,sans-serif;margin:24px;max-width:760px}}table{{border-collapse:collapse}}td,th{{border:1px solid #ddd;padding:4px 8px;text-align:right}}th{{text-align:left}}svg{{border:1px solid #eee;margin:8px 0}}h2{{margin-top:24px;margin-bottom:4px}}</style></head><body>");
    let _ = write!(s, "<h1>rquant factor workbench</h1>");

    // ── headline 参数表 ──────────────────────────────────────────────────────
    let _ = write!(s, "<table><tr><th>param</th><th>value</th></tr>");
    let _ = write!(s, "<tr><th>n_symbols</th><td>{}</td></tr>", report.n_symbols);
    let _ = write!(s, "<tr><th>n_sample_points</th><td>{}</td></tr>", report.n_sample_points);
    let _ = write!(s, "<tr><th>sample_K</th><td>{}</td></tr>", report.sample);
    let _ = write!(s, "<tr><th>horizon_H</th><td>{}</td></tr>", report.horizon);
    let _ = write!(s, "<tr><th>layers_Q</th><td>{}</td></tr>", report.layers_q);
    let _ = write!(s, "</table>");

    // ── 因子汇总表（每因子一行）────────────────────────────────────────────
    let _ = write!(s, "<h2>Factor Summary</h2>");
    let _ = write!(s, "<table><tr><th>name</th><th>expr</th><th>n_periods</th><th>n_skipped</th><th>RankIC</th><th>ICIR</th><th>monotonicity</th><th>spread_total</th><th>spread_ann</th><th>spread_Sharpe</th></tr>");
    for fs in &report.factors {
        let mono = fs.layers.as_ref().and_then(|ls| ls.monotonicity);
        let spread_total = fs.layers.as_ref().map(|ls| Some(ls.spread_total));
        let spread_ann = fs.layers.as_ref().and_then(|ls| ls.spread_ann);
        let spread_sharpe = fs.layers.as_ref().and_then(|ls| ls.spread_sharpe);
        let _ = write!(s,
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            fs.name, fs.expr,
            fs.n_periods, fs.n_skipped,
            opt_fmt(fs.rank_ic_mean),
            opt_fmt2(fs.rank_icir),
            opt_fmt2(mono),
            spread_total.map_or_else(|| "—".to_string(), opt_fmt),
            opt_fmt(spread_ann),
            opt_fmt2(spread_sharpe),
        );
    }
    let _ = write!(s, "</table>");

    // ── IC 衰减多线图（每因子一线，x=阶梯序，y=mean RankIC，None 点跳过）─
    let _ = write!(s, "<h2>IC Decay (RankIC by horizon)</h2>");
    let decay_series: Vec<(String, Vec<(f64, f64)>)> = report
        .factors
        .iter()
        .map(|fs| {
            let pts: Vec<(f64, f64)> = fs
                .ic_decay
                .iter()
                .enumerate()
                .filter_map(|(i, (_, v))| v.map(|y| (i as f64, y)))
                .collect();
            (fs.name.clone(), pts)
        })
        .collect();
    let _ = write!(s, "{}", multi_line_chart(&decay_series, "IC Decay: mean RankIC per ladder step"));

    // ── 逐因子分层年化条形图 ────────────────────────────────────────────────
    let _ = write!(s, "<h2>Layer Annual Returns (low → high factor quantile)</h2>");
    for fs in &report.factors {
        if let Some(ls) = &fs.layers {
            let items: Vec<(String, f64)> = ls
                .ann_returns
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let label = format!("Q{}", i + 1);
                    let val = v.unwrap_or(0.0);
                    (label, val)
                })
                .collect();
            let has_none = ls.ann_returns.iter().any(|v| v.is_none());
            let title = if has_none {
                format!("{}: layer ann_return (None→0)", fs.name)
            } else {
                format!("{}: layer ann_return", fs.name)
            };
            let _ = write!(s, "{}", bar_chart(&items, &title));
        } else {
            let _ = write!(s, "<p>{}: no layer data</p>", fs.name);
        }
    }

    // ── 逐因子 spread 净值曲线 ──────────────────────────────────────────────
    let _ = write!(s, "<h2>Spread NAV (top layer minus bottom layer)</h2>");
    for fs in &report.factors {
        if let Some(ls) = &fs.layers {
            if ls.spread_nav.len() >= 2 {
                let pts: Vec<(f64, f64)> = ls.spread_nav.iter().enumerate().map(|(i, (_, nav))| (i as f64, *nav)).collect();
                let title = format!("{}: spread NAV curve", fs.name);
                let _ = write!(s, "{}", line_chart(&pts, &title));
            } else {
                let _ = write!(s, "<p>{}: spread NAV series too short (length {})</p>", fs.name, ls.spread_nav.len());
            }
        } else {
            let _ = write!(s, "<p>{}: no spread data</p>", fs.name);
        }
    }

    // ── 相关性矩阵 HTML 表 ──────────────────────────────────────────────────
    if let Some(corr) = &report.corr {
        let _ = write!(s, "<h2>Factor Correlation Matrix</h2>");
        let _ = write!(s, "<table><tr><th></th>");
        for name in &corr.names {
            let _ = write!(s, "<th>{name}</th>");
        }
        let _ = write!(s, "</tr>");
        for (i, row) in corr.values.iter().enumerate() {
            let _ = write!(s, "<tr><th>{}</th>", corr.names[i]);
            for v in row {
                let _ = write!(s, "<td>{}</td>", opt_fmt2(*v));
            }
            let _ = write!(s, "</tr>");
        }
        let _ = write!(s, "</table>");
    }

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
        Report { tree_name: "viz".into(), forward_window: 8, cost_bps: 10.0, metrics, gaps: GapReport::default(), walk_forward: None }
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
            position: signal_stat(&[0.1, 0.2]),
            buy_and_hold: 0.05,
            overlap_warning: "OVLAP".into(),
        };
        let t = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        let report = SoftReport {
            tree_name: "softviz".into(),
            forward_window: 4,
            cost_bps: 10.0,
            soft,
            walk_forward: Some(crate::backtest::walkforward::WalkForward {
                folds: vec![crate::backtest::walkforward::FoldMetrics {
                    from: t, to: t, stat: signal_stat(&[0.1]), buy_and_hold: 0.0,
                }],
                positive_folds: 1,
                worst_mean_net: 0.1,
            }),
        };
        let series = EquitySeries {
            points: vec![SeriesPoint { t, net: 0.1, cum: 0.1 }, SeriesPoint { t, net: 0.2, cum: 0.3 }],
            hist: Histogram { bins: vec![(0.0, 0.2, 2)] },
            skipped: 0,
        };
        let avg = vec![("leaf_l".to_string(), 0.7), ("leaf_f".to_string(), 0.3)];
        let st = crate::report::curve::StackSeries {
            names: vec!["leaf_l".to_string()],
            rows: vec![vec![1.0]],
        };
        let a = render_soft_html(&report, &series, &avg, Some(&st));
        let b = render_soft_html(&report, &series, &avg, Some(&st));
        assert_eq!(a, b);
        assert!(a.contains("<!doctype html>"));
        assert!(a.contains("softviz"));
        assert!(a.contains("<polyline"));
        assert!(a.contains("<rect"));
        assert!(a.contains("OVLAP"));
        assert!(a.contains("<polygon"));
        assert!(a.contains("walk-forward"));
    }

    #[test]
    fn stacked_area_chart_has_polygons_and_legend() {
        use crate::report::curve::StackSeries;
        let st = StackSeries {
            names: vec!["leaf_a".to_string(), "leaf_b".to_string()],
            rows: vec![vec![0.3, 1.0], vec![0.6, 1.0], vec![0.5, 1.0]],
        };
        let svg = stacked_area_chart(&st, "t");
        assert!(svg.contains("<polygon"));
        assert!(svg.contains("leaf_a"));
        assert!(svg.contains("leaf_b"));
        assert_eq!(svg, stacked_area_chart(&st, "t")); // 确定性
    }

    #[test]
    fn render_sim_html_with_and_without_steps() {
        use crate::backtest::sim::{RoundTrip, SimReport, SimStepRecord};
        use chrono::NaiveDate;
        let t0 = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(10, 0, 0).unwrap();
        let trip = |r: f64| RoundTrip {
            entry_t: t0, exit_t: t0, entry_px: 10.0, exit_px: 10.0,
            max_abs_pos: 1.0, trip_return: r, bars_held: 2, reason: "tree".into(),
        };
        let rep = SimReport {
            tree_name: "simviz".into(), cost_bps: 10.0, total_return: 0.1, max_drawdown: 0.05,
            n_round_trips: 2, win_rate: 0.5, avg_hold_bars: 2.0, turnover: 4.0, buy_and_hold: 0.02,
            trades: vec![trip(0.05), trip(-0.01)],
            risk: None,
        };
        let steps = vec![
            SimStepRecord { t: t0, target: 1.0, pos: 1.0, nav: 1.0 },
            SimStepRecord { t: t0, target: 1.0, pos: 1.0, nav: 1.02 },
        ];
        let a = render_sim_html(&rep, Some(&steps));
        assert_eq!(a, render_sim_html(&rep, Some(&steps))); // 确定性
        assert!(a.contains("<!doctype html>") && a.contains("simviz"));
        assert!(a.contains("<polyline")); // 净值曲线
        assert!(a.contains("<rect"));     // 直方图
        assert!(a.contains("tree"));      // 回合表
        let b = render_sim_html(&rep, None);
        assert!(!b.contains("<polyline") && b.contains("未提供")); // 占位
    }

    #[test]
    fn render_portfolio_html_self_contained() {
        use crate::backtest::portfolio::{HoldingsRecord, PortfolioReport};
        use chrono::NaiveDate;
        let t0 = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(10, 0, 0).unwrap();
        let h = |nav: f64, b: f64, sel: &str| HoldingsRecord {
            t: t0, nav, benchmark_nav: b, selected: vec![(sel.to_string(), 1.0)],
        };
        let rep = PortfolioReport {
            tree_name: "pfviz".into(), cost_bps: 10.0, top_n: 1, rebalance: 4,
            n_rebalances: 3, avg_members: 1.0, total_return: 0.06, max_drawdown: 0.02,
            turnover: 3.0, benchmark_return: 0.01,
            holdings: vec![h(1.0, 1.0, "A"), h(1.03, 1.0, "A"), h(1.06, 1.01, "B")],
            risk: None,
        };
        let a = render_portfolio_html(&rep);
        assert_eq!(a, render_portfolio_html(&rep));
        assert!(a.contains("pfviz"));
        assert_eq!(a.matches("<polyline").count(), 2); // 组合+基准双线
        assert!(a.contains("<rect"));                  // 频率条形（A:2/3, B:1/3）+ 图例 rect
        assert!(a.contains(">A<") || a.contains("A")); // 持仓表
    }

    #[test]
    fn multi_line_chart_two_lines_with_legend() {
        let series = vec![
            ("组合".to_string(), vec![(0.0, 1.0), (1.0, 1.05), (2.0, 1.02)]),
            ("基准".to_string(), vec![(0.0, 1.0), (1.0, 0.98), (2.0, 0.99)]),
        ];
        let svg = multi_line_chart(&series, "t");
        assert_eq!(svg.matches("<polyline").count(), 2);
        assert!(svg.contains("组合") && svg.contains("基准"));
        assert_eq!(svg, multi_line_chart(&series, "t")); // 确定性
    }

    fn sample_factor_report(n_factors: usize) -> crate::factor::FactorReport {
        use crate::factor::{FactorReport, FactorStats, LayerStats, CorrMatrix};
        let make_fs = |name: &str, expr: &str| FactorStats {
            name: name.to_string(),
            expr: expr.to_string(),
            n_periods: 5,
            n_skipped: 1,
            ic_mean: Some(0.12),
            ic_std: Some(0.05),
            icir: Some(2.4),
            ic_t: Some(2.1),
            ic_pos_share: Some(0.8),
            rank_ic_mean: Some(0.11),
            rank_ic_std: Some(0.04),
            rank_icir: Some(2.75),
            rank_ic_t: Some(2.3),
            rank_ic_pos_share: Some(0.8),
            ic_decay: vec![(4, Some(0.11)), (8, Some(0.09)), (16, Some(0.07)), (32, Some(0.05)), (64, Some(0.03))],
            layers: Some(LayerStats {
                q: 5,
                ann_returns: vec![Some(-0.1), Some(0.02), Some(0.08), Some(0.15), Some(0.22)],
                spread_total: 0.35,
                spread_ann: Some(0.18),
                spread_sharpe: Some(1.5),
                monotonicity: Some(0.95),
                spread_nav: vec![
                    (chrono::NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 30, 0).unwrap(), 1.0),
                    (chrono::NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(10, 0, 0).unwrap(), 1.05),
                    (chrono::NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(10, 30, 0).unwrap(), 1.12),
                    (chrono::NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(11, 0, 0).unwrap(), 1.15),
                    (chrono::NaiveDate::from_ymd_opt(2024, 1, 3).unwrap().and_hms_opt(9, 30, 0).unwrap(), 1.20),
                    (chrono::NaiveDate::from_ymd_opt(2024, 1, 3).unwrap().and_hms_opt(10, 0, 0).unwrap(), 1.35),
                ],
            }),
        };
        let factors = if n_factors == 1 {
            vec![make_fs("mom", "close/ref(close,4)-1")]
        } else {
            vec![
                make_fs("mom", "close/ref(close,4)-1"),
                make_fs("rev", "ref(close,4)/close-1"),
            ]
        };
        let corr = if n_factors >= 2 {
            Some(CorrMatrix {
                names: vec!["mom".to_string(), "rev".to_string()],
                values: vec![
                    vec![Some(1.0), Some(-0.95)],
                    vec![Some(-0.95), Some(1.0)],
                ],
            })
        } else {
            None
        };
        FactorReport {
            n_symbols: 6,
            n_sample_points: 8,
            sample: 4,
            horizon: 16,
            layers_q: 5,
            factors,
            corr,
        }
    }

    #[test]
    fn render_factor_html_deterministic() {
        let report = sample_factor_report(2);
        let a = render_factor_html(&report);
        let b = render_factor_html(&report);
        assert_eq!(a, b, "render_factor_html must be deterministic");
    }

    #[test]
    fn render_factor_html_contains_rank_ic() {
        let report = sample_factor_report(1);
        let html = render_factor_html(&report);
        assert!(html.contains("RankIC"), "HTML must contain 'RankIC'");
        assert!(html.contains("<!doctype html>"), "must be a valid HTML document");
    }

    #[test]
    fn render_factor_html_polyline_count_ge_n_factors() {
        // 2 factors: decay multi_line_chart has ≥2 polylines
        let report = sample_factor_report(2);
        let html = render_factor_html(&report);
        let n_factors = report.factors.len();
        let polyline_count = html.matches("<polyline").count();
        assert!(
            polyline_count >= n_factors,
            "polyline count ({polyline_count}) should be >= n_factors ({n_factors})"
        );
    }

    #[test]
    fn render_factor_html_corr_table_present_when_multi_factor() {
        let report = sample_factor_report(2);
        let html = render_factor_html(&report);
        assert!(html.contains("correlation") || html.contains("Correlation"), "corr section should be present");
        // Both factor names appear
        assert!(html.contains("mom") && html.contains("rev"));
    }

    #[test]
    fn render_factor_html_single_factor_no_corr_table() {
        let report = sample_factor_report(1);
        let html = render_factor_html(&report);
        // Single factor: no corr matrix rendered
        assert!(!html.contains("correlation") && !html.contains("Correlation"),
            "single-factor report should have no correlation table");
    }
}
