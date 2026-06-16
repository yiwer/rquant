pub mod stats;

use crate::backtest::costs::CostModel;
use crate::backtest::forward_return::forward_return;
use crate::backtest::portfolio::build_timeline;
use crate::data::fundamentals::{FundamentalSeries, load_fundamentals_csv};
use crate::data::universe::read_universe_csv;
use crate::dsl::parser::parse_str;
use crate::dsl::eval::eval_scalar;
use crate::features::context::build_context;
use crate::report::risk;
use crate::tree::schema::Stance;
use crate::{Error, Result};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

// ─────────────────────────────────────────────────────────────────────────────
// 公开类型
// ─────────────────────────────────────────────────────────────────────────────

pub struct FactorSpecItem {
    pub name: String,
    pub expr: String,
}

pub struct FactorConfig {
    pub universe_path: PathBuf,
    pub factors: Vec<FactorSpecItem>,
    pub sample: usize,   // 采样间隔 K
    pub horizon: usize,  // 主前瞻 H
    pub layers: usize,   // Q
    pub warmup: usize,
    pub window: usize,
    pub out_path: PathBuf,
    pub html_path: Option<PathBuf>,
    /// 可选点时 universe 成员 CSV（date,symbol）；每截面只取该 t 生效成员。None=不过滤（行为冻结）。
    pub membership_path: Option<PathBuf>,
}

/// 一个采样期的原始观测：每标的（因子值按因子序、收益按阶梯序对齐）。
#[derive(Debug)]
pub(crate) struct SymbolPoint {
    pub symbol: String,
    pub factors: Vec<Option<f64>>, // 非有限 → None
    pub rets: Vec<Option<f64>>,    // forward_return gross；尾部不足 → None
}

#[derive(Debug)]
pub(crate) struct PeriodData {
    pub t: NaiveDateTime,
    pub points: Vec<SymbolPoint>,
}

// ─────────────────────────────────────────────────────────────────────────────
// 报告类型
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct LayerStats {
    pub q: usize,
    pub ann_returns: Vec<Option<f64>>, // 低→高因子层
    pub spread_total: f64,             // top−bottom 连乘净值 −1
    pub spread_ann: Option<f64>,
    pub spread_sharpe: Option<f64>,
    pub monotonicity: Option<f64>, // spearman(层序号, 层期均收益)
    pub spread_nav: Vec<(chrono::NaiveDateTime, f64)>, // spread nav accumulation series
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FactorStats {
    pub name: String,
    pub expr: String,
    pub n_periods: usize,  // 进入 IC 统计的有效期数
    pub n_skipped: usize,  // 有效对 < max(Q,5) 被跳过的期数
    pub ic_mean: Option<f64>,
    pub ic_std: Option<f64>,
    pub icir: Option<f64>,
    pub ic_t: Option<f64>,
    pub ic_pos_share: Option<f64>,
    pub rank_ic_mean: Option<f64>,
    pub rank_ic_std: Option<f64>,
    pub rank_icir: Option<f64>,
    pub rank_ic_t: Option<f64>,
    pub rank_ic_pos_share: Option<f64>,
    pub ic_decay: Vec<(usize, Option<f64>)>, // (horizon, mean RankIC)
    pub layers: Option<LayerStats>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CorrMatrix {
    pub names: Vec<String>,
    pub values: Vec<Vec<Option<f64>>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FactorReport {
    pub n_symbols: usize,
    pub n_sample_points: usize,
    pub sample: usize,
    pub horizon: usize,
    pub layers_q: usize,
    pub factors: Vec<FactorStats>,
    pub corr: Option<CorrMatrix>,
}

// ─────────────────────────────────────────────────────────────────────────────
// 采样循环
// ─────────────────────────────────────────────────────────────────────────────

/// 横截面采样：对每个采样点 t 和每个标的收集因子值 + 前瞻收益阶梯。
///
/// 返回 `(periods, ladder, n_symbols)`：
/// - `periods`：每采样时刻一个 PeriodData（即使全 None 也保留）
/// - `ladder`：IC 衰减阶梯（horizon 的 h/4,h/2,h,2h,4h 去重升序）
/// - `n_symbols`：universe 标的数
pub(crate) fn collect_periods(
    cfg: &FactorConfig,
) -> Result<(Vec<PeriodData>, Vec<usize>, usize)> {
    // ── 1. 校验 ──────────────────────────────────────────────────────────────
    if cfg.factors.is_empty() {
        return Err(Error::Data("factor: factors list must not be empty".into()));
    }
    // name 唯一非空
    let mut seen_names: std::collections::BTreeSet<&str> = Default::default();
    for f in &cfg.factors {
        if f.name.is_empty() {
            return Err(Error::Data("factor: factor name must not be empty".into()));
        }
        if !seen_names.insert(f.name.as_str()) {
            return Err(Error::Data(format!(
                "factor: duplicate factor name '{}'",
                f.name
            )));
        }
    }
    if cfg.sample < 1 {
        return Err(Error::Data("factor: sample must be >= 1".into()));
    }
    if cfg.layers < 1 {
        return Err(Error::Data("factor: layers must be >= 1".into()));
    }
    if cfg.horizon < 1 {
        return Err(Error::Data("factor: horizon must be >= 1".into()));
    }

    // 预解析所有因子表达式（加载期校验，含因子名）
    let parsed_exprs: Vec<crate::dsl::ast::Expr> = cfg
        .factors
        .iter()
        .map(|f| {
            parse_str(&f.expr).map_err(|e| {
                Error::Data(format!("factor '{}': DSL parse error: {}", f.name, e))
            })
        })
        .collect::<Result<Vec<_>>>()?;

    // ── 2. universe 加载 ─────────────────────────────────────────────────────
    let universe = read_universe_csv(&cfg.universe_path)?;
    let n_symbols = universe.len();

    // 点时成员（None=不过滤=行为冻结）
    let membership = cfg
        .membership_path
        .as_ref()
        .map(|p| crate::data::membership::Membership::load_csv(p))
        .transpose()?;

    let mut primaries: Vec<Vec<crate::data::bar::Bar>> = Vec::with_capacity(n_symbols);
    let mut contexts: Vec<Vec<crate::data::bar::Bar>> = Vec::with_capacity(n_symbols);
    let mut funds: Vec<Option<FundamentalSeries>> = Vec::with_capacity(n_symbols);
    for entry in &universe {
        primaries.push(crate::data::reader::read_bars_csv(&entry.primary)?);
        contexts.push(crate::data::reader::read_bars_csv(&entry.context)?);
        funds.push(
            entry
                .fundamentals
                .as_ref()
                .map(|p| load_fundamentals_csv(p))
                .transpose()?,
        );
    }

    // ── 3. 时间线 + 采样索引 ─────────────────────────────────────────────────
    let timeline = build_timeline(&primaries);
    let n = timeline.len();

    let sample_indices: Vec<usize> = (cfg.warmup..n).step_by(cfg.sample).collect();
    if sample_indices.len() < 2 {
        return Err(Error::Data(
            "factor: universe timeline too short for warmup/sample (need >= 2 sample points)"
                .into(),
        ));
    }

    // ── 4. IC 衰减阶梯 ───────────────────────────────────────────────────────
    let ladder = stats::decay_ladder(cfg.horizon);

    // ── 5. 零成本模型 ────────────────────────────────────────────────────────
    let zero_cost = CostModel { round_trip_bps: 0.0 };
    let empty_aux: BTreeMap<String, crate::data::aux_table::AuxTable> = BTreeMap::new();

    // ── 6. 采样循环 ──────────────────────────────────────────────────────────
    let mut periods: Vec<PeriodData> = Vec::with_capacity(sample_indices.len());

    for &ti in &sample_indices {
        let t = timeline[ti];
        // 当期生效成员：None=未配置(不过滤)；Some(None)=配置但 t 早于首期(空截面)；Some(Some(set))=限定
        let eff = membership.as_ref().map(|m| m.effective_at(t));
        let mut points: Vec<SymbolPoint> = Vec::with_capacity(n_symbols);

        for (sym_idx, entry) in universe.iter().enumerate() {
            let bars = &primaries[sym_idx];
            let ctx_bars = &contexts[sym_idx];

            // 仅恰有 bar 在 t 的标的参与（is_fresh 的索引版）
            let bar_i = match bars.binary_search_by_key(&t, |b| b.time) {
                Ok(i) => i,
                Err(_) => continue, // 该标的在此时刻无 bar
            };

            // membership mask（point-in-time）
            match eff {
                None => {}                                              // 未配置 → 保留
                Some(Some(set)) if set.contains(&entry.symbol) => {}    // 成员 → 保留
                _ => continue,                                          // 配置但空/非成员 → 跳过
            }

            // 构建 context（news 空、aux 空；基本面取该标的 as-of-t 快照）
            let ctx = build_context(
                bars,
                ctx_bars,
                &[],
                &empty_aux,
                funds[sym_idx].as_ref(),
                t,
                cfg.window,
            );

            // 逐因子 eval_scalar（Err 或非有限 → None）
            let factor_vals: Vec<Option<f64>> = parsed_exprs
                .iter()
                .map(|expr| {
                    match eval_scalar(expr, &ctx) {
                        Ok(v) if v.is_finite() => Some(v),
                        _ => None,
                    }
                })
                .collect();

            // 逐阶梯 h forward_return gross（None → None；零成本）
            let rets: Vec<Option<f64>> = ladder
                .iter()
                .map(|&h| {
                    forward_return(bars, bar_i, h, Stance::Long, &zero_cost)
                        .map(|fr| fr.gross)
                })
                .collect();

            points.push(SymbolPoint {
                symbol: entry.symbol.clone(),
                factors: factor_vals,
                rets,
            });
        }

        periods.push(PeriodData { t, points });
    }

    Ok((periods, ladder, n_symbols))
}

// ─────────────────────────────────────────────────────────────────────────────
// 聚合核心
// ─────────────────────────────────────────────────────────────────────────────

/// 汇总统计元组：(mean, std, icir, t, pos_share)，全 Option<f64>。
type SummaryTuple = (Option<f64>, Option<f64>, Option<f64>, Option<f64>, Option<f64>);

/// 从 IC/RankIC 序列计算汇总统计（mean/std/ICIR/t/pos_share）。
/// 序列为空时全返回 None。
fn summarise(series: &[f64]) -> SummaryTuple {
    if series.is_empty() {
        return (None, None, None, None, None);
    }
    let n = series.len() as f64;
    let mean = series.iter().sum::<f64>() / n;
    let std = if series.len() >= 2 {
        let var = series.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1.0);
        Some(var.sqrt())
    } else {
        None
    };
    let icir = std.filter(|&s| s > 1e-12).map(|s| mean / s);
    let t = risk::t_stat(series);
    let pos = series.iter().filter(|&&v| v > 0.0).count() as f64 / n;
    (Some(mean), std, icir, t, Some(pos))
}

/// 横截面因子检验主入口：计算 IC 汇总、IC 衰减、分层回测、相关性矩阵。
pub fn run_factor(cfg: &FactorConfig) -> Result<FactorReport> {
    let (periods, ladder, n_symbols) = collect_periods(cfg)?;
    let n_factors = cfg.factors.len();
    let q = cfg.layers;

    // 主档在 ladder 中的索引（horizon 一定在 ladder 中）
    let main_h_idx = ladder
        .iter()
        .position(|&h| h == cfg.horizon)
        .expect("horizon must be in decay ladder");

    // 阈值：有效对数 < max(Q, 5) 则跳过
    let skip_threshold = q.max(5);

    // ── 逐因子聚合 ────────────────────────────────────────────────────────────
    let mut factor_stats_vec: Vec<FactorStats> = Vec::with_capacity(n_factors);

    for f_idx in 0..n_factors {
        let mut ic_series: Vec<f64> = Vec::new();
        let mut rank_ic_series: Vec<f64> = Vec::new();
        let mut n_skipped: usize = 0;

        // 分层状态：每层维护 (nav, nav_points, period_returns)
        // nav_points: Vec<(NaiveDateTime, f64)>
        // period_returns: Vec<f64> — 每期该层均收益，用于单调性计算
        let mut layer_navs: Vec<f64> = vec![1.0; q];
        let mut layer_nav_points: Vec<Vec<(NaiveDateTime, f64)>> = vec![vec![]; q];
        let mut layer_period_rets: Vec<Vec<f64>> = vec![vec![]; q]; // per-layer, per-period ret

        // 初始 nav 点（时间戳用第一个采样点，但我们仅在有效期才 push）
        // spread 状态
        let mut spread_nav: f64 = 1.0;
        let mut spread_nav_points: Vec<(NaiveDateTime, f64)> = Vec::new();
        let mut spread_peak: f64 = 1.0;
        let mut spread_max_dd: f64 = 0.0;

        // 逐期处理（decay 独立维护）
        // decay: ladder_idx → (period_rank_ic_vals)
        let mut decay_rank_ic: Vec<Vec<f64>> = vec![vec![]; ladder.len()];

        for period in &periods {
            // 收集主档有效对 (factor_val, ret)，同时保持 symbol 排序确定性
            let mut valid_pairs: Vec<(String, f64, f64)> = period
                .points
                .iter()
                .filter_map(|pt| {
                    let fv = pt.factors[f_idx]?;
                    let rv = pt.rets[main_h_idx]?;
                    Some((pt.symbol.clone(), fv, rv))
                })
                .collect();

            let count = valid_pairs.len();
            if count < skip_threshold {
                n_skipped += 1;
                // 衰减阶梯也跳过（独立有效性在下方）
            } else {
                // 按因子升序，symbol 作次键保证确定性
                valid_pairs.sort_by(|a, b| {
                    a.1.partial_cmp(&b.1)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| a.0.cmp(&b.0))
                });

                let fvals: Vec<f64> = valid_pairs.iter().map(|(_, f, _)| *f).collect();
                let rvals: Vec<f64> = valid_pairs.iter().map(|(_, _, r)| *r).collect();

                // IC（Pearson）
                if let Some(ic) = stats::pearson(&fvals, &rvals) {
                    ic_series.push(ic);
                }
                // RankIC（Spearman）
                if let Some(ric) = stats::spearman(&fvals, &rvals) {
                    rank_ic_series.push(ric);
                }

                // 分层
                let sizes = stats::layer_sizes(count, q);
                let mut offset = 0usize;
                for (l_idx, &sz) in sizes.iter().enumerate() {
                    let members = &valid_pairs[offset..offset + sz];
                    offset += sz;
                    let layer_ret = members.iter().map(|(_, _, r)| *r).sum::<f64>() / sz as f64;
                    layer_period_rets[l_idx].push(layer_ret);
                    layer_navs[l_idx] *= 1.0 + layer_ret;
                    layer_nav_points[l_idx].push((period.t, layer_navs[l_idx]));
                }

                // spread = top − bottom
                let bottom_ret = layer_period_rets[0].last().copied().unwrap_or(0.0);
                let top_ret = layer_period_rets[q - 1].last().copied().unwrap_or(0.0);
                spread_nav *= 1.0 + (top_ret - bottom_ret);
                if spread_nav > spread_peak {
                    spread_peak = spread_nav;
                }
                let dd = (spread_peak - spread_nav) / spread_peak;
                if dd > spread_max_dd {
                    spread_max_dd = dd;
                }
                spread_nav_points.push((period.t, spread_nav));
            }

            // 衰减：逐阶梯独立有效性（≥5 共同有效对才计）
            for (l_idx, &h) in ladder.iter().enumerate() {
                // 找该 ladder slot 对应 ret 索引就是 l_idx
                let valid_decay: Vec<(f64, f64)> = period
                    .points
                    .iter()
                    .filter_map(|pt| {
                        let fv = pt.factors[f_idx]?;
                        let rv = pt.rets[l_idx]?;
                        Some((fv, rv))
                    })
                    .collect();
                if valid_decay.len() >= 5 {
                    let df: Vec<f64> = valid_decay.iter().map(|(f, _)| *f).collect();
                    let dr: Vec<f64> = valid_decay.iter().map(|(_, r)| *r).collect();
                    if let Some(ric) = stats::spearman(&df, &dr) {
                        decay_rank_ic[l_idx].push(ric);
                    }
                }
                // h is used for ic_decay output below
                let _ = h;
            }
        }

        let n_periods = rank_ic_series.len().max(ic_series.len());

        // IC 汇总
        let (ic_mean, ic_std, icir, ic_t, ic_pos_share) = summarise(&ic_series);
        let (rank_ic_mean, rank_ic_std, rank_icir, rank_ic_t, rank_ic_pos_share) =
            summarise(&rank_ic_series);

        // IC 衰减
        let ic_decay: Vec<(usize, Option<f64>)> = ladder
            .iter()
            .zip(decay_rank_ic.iter())
            .map(|(&h, series)| {
                let mean = if series.is_empty() {
                    None
                } else {
                    Some(series.iter().sum::<f64>() / series.len() as f64)
                };
                (h, mean)
            })
            .collect();

        // 分层年化
        let layers_stat = if n_periods > 0 && !layer_nav_points[0].is_empty() {
            // 每层年化 ann_return
            let ann_returns: Vec<Option<f64>> = layer_nav_points
                .iter()
                .map(|pts| {
                    risk::risk_metrics(pts, 0.0).and_then(|m| m.ann_return)
                })
                .collect();

            // spread ann/sharpe
            let (spread_ann, spread_sharpe) = if spread_nav_points.len() >= 2 {
                match risk::risk_metrics(&spread_nav_points, spread_max_dd) {
                    Some(m) => (m.ann_return, m.sharpe),
                    None => (None, None),
                }
            } else {
                (None, None)
            };

            let spread_total = spread_nav - 1.0;

            // 单调性：spearman(层序号 0..Q as f64, 各层期均收益)
            let layer_mean_rets: Vec<f64> = layer_period_rets
                .iter()
                .map(|pr| {
                    if pr.is_empty() {
                        0.0
                    } else {
                        pr.iter().sum::<f64>() / pr.len() as f64
                    }
                })
                .collect();
            let layer_indices: Vec<f64> = (0..q).map(|i| i as f64).collect();
            let monotonicity = stats::spearman(&layer_indices, &layer_mean_rets);

            Some(LayerStats {
                q,
                ann_returns,
                spread_total,
                spread_ann,
                spread_sharpe,
                monotonicity,
                spread_nav: spread_nav_points.clone(),
            })
        } else {
            None
        };

        factor_stats_vec.push(FactorStats {
            name: cfg.factors[f_idx].name.clone(),
            expr: cfg.factors[f_idx].expr.clone(),
            n_periods,
            n_skipped,
            ic_mean,
            ic_std,
            icir,
            ic_t,
            ic_pos_share,
            rank_ic_mean,
            rank_ic_std,
            rank_icir,
            rank_ic_t,
            rank_ic_pos_share,
            ic_decay,
            layers: layers_stat,
        });
    }

    // ── 相关性矩阵（≥2 因子）────────────────────────────────────────────────
    let corr = if n_factors >= 2 {
        let names: Vec<String> = cfg.factors.iter().map(|f| f.name.clone()).collect();
        let mut values: Vec<Vec<Option<f64>>> = vec![vec![None; n_factors]; n_factors];

        // 对角恒 Some(1.0)
        for (i, row) in values.iter_mut().enumerate() {
            row[i] = Some(1.0);
        }

        // 每对因子 (i, j) i < j：逐期共同 Some 标的 spearman，然后各期均值
        for i in 0..n_factors {
            for j in (i + 1)..n_factors {
                let mut period_corrs: Vec<f64> = Vec::new();
                for period in &periods {
                    // 共同 Some 标的
                    let common: Vec<(f64, f64)> = period
                        .points
                        .iter()
                        .filter_map(|pt| {
                            let fi = pt.factors[i]?;
                            let fj = pt.factors[j]?;
                            Some((fi, fj))
                        })
                        .collect();
                    if common.len() >= 5 {
                        let xi: Vec<f64> = common.iter().map(|(a, _)| *a).collect();
                        let xj: Vec<f64> = common.iter().map(|(_, b)| *b).collect();
                        if let Some(r) = stats::spearman(&xi, &xj) {
                            period_corrs.push(r);
                        }
                    }
                }
                let avg = if period_corrs.is_empty() {
                    None
                } else {
                    Some(period_corrs.iter().sum::<f64>() / period_corrs.len() as f64)
                };
                values[i][j] = avg;
                values[j][i] = avg;
            }
        }

        Some(CorrMatrix { names, values })
    } else {
        None
    };

    // ── 组装报告 + 写 JSON ────────────────────────────────────────────────────
    let report = FactorReport {
        n_symbols,
        n_sample_points: periods.len(),
        sample: cfg.sample,
        horizon: cfg.horizon,
        layers_q: q,
        factors: factor_stats_vec,
        corr,
    };

    let json = serde_json::to_string_pretty(&report)?;
    std::fs::write(&cfg.out_path, json)?;

    Ok(report)
}

// ─────────────────────────────────────────────────────────────────────────────
// 打印摘要
// ─────────────────────────────────────────────────────────────────────────────

/// 打印因子检验摘要到 stdout（格式匹配既有 print_summary 风格）。
pub fn print_factor_summary(report: &FactorReport) {
    let fmt_opt = |v: Option<f64>| v.map_or("—".to_string(), |x| format!("{:.4}", x));
    let fmt_opt2 = |v: Option<f64>| v.map_or("—".to_string(), |x| format!("{:.2}", x));

    println!(
        "=== rquant factor workbench: {} factor(s), sample={}, horizon={}, layers={} ===",
        report.factors.len(),
        report.sample,
        report.horizon,
        report.layers_q,
    );
    println!("n_symbols={} n_sample_points={}", report.n_symbols, report.n_sample_points);

    for fs in &report.factors {
        println!("─── factor: {} [{}] ─────────────────────────────", fs.name, fs.expr);
        println!("  n_periods={} n_skipped={}", fs.n_periods, fs.n_skipped);
        println!(
            "  RankIC  mean={} ICIR={} t={} pos%={}",
            fmt_opt(fs.rank_ic_mean),
            fmt_opt2(fs.rank_icir),
            fmt_opt2(fs.rank_ic_t),
            fs.rank_ic_pos_share.map_or("—".to_string(), |x| format!("{:.1}%", x * 100.0)),
        );
        println!(
            "  IC      mean={} ICIR={} t={} pos%={}",
            fmt_opt(fs.ic_mean),
            fmt_opt2(fs.icir),
            fmt_opt2(fs.ic_t),
            fs.ic_pos_share.map_or("—".to_string(), |x| format!("{:.1}%", x * 100.0)),
        );
        // IC 衰减一行
        let decay_str: Vec<String> = fs
            .ic_decay
            .iter()
            .map(|(h, v)| format!("h={}:{}", h, fmt_opt(*v)))
            .collect();
        println!("  decay   {}", decay_str.join(" "));

        // 分层
        if let Some(ls) = &fs.layers {
            let ann_str: Vec<String> = ls
                .ann_returns
                .iter()
                .enumerate()
                .map(|(i, v)| format!("Q{}:{}", i + 1, fmt_opt(*v)))
                .collect();
            println!(
                "  layers  {} → spread total={} ann={} Sharpe={}",
                ann_str.join(" "),
                fmt_opt(Some(ls.spread_total)),
                fmt_opt(ls.spread_ann),
                fmt_opt2(ls.spread_sharpe),
            );
            println!("  monotonicity={}", fmt_opt2(ls.monotonicity));
        } else {
            println!("  layers  —");
        }
    }

    // 相关性矩阵
    if let Some(corr) = &report.corr {
        println!("─── correlation matrix ─────────────────────────────────");
        // 标题行
        let header: Vec<String> = corr.names.iter().map(|n| format!("{:>8}", n)).collect();
        println!("         {}", header.join(" "));
        for (i, row) in corr.values.iter().enumerate() {
            let cells: Vec<String> = row
                .iter()
                .map(|v| format!("{:>8}", fmt_opt2(*v)))
                .collect();
            println!("  {:>6}  {}", corr.names[i], cells.join(" "));
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use std::io::Write as _;

    /// 写出价格序列为 CSV bar 文件（tempfile）。
    /// p_t = start * (1 + g)^t，所有 OHLC 等于 close（简单价格序列）。
    fn write_price_csv(
        timestamps: &[NaiveDateTime],
        start: f64,
        g: f64,
    ) -> tempfile::NamedTempFile {
        let mut f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        writeln!(f, "time,open,high,low,close,volume").unwrap();
        let mut price = start;
        for ts in timestamps {
            writeln!(
                f,
                "{},{:.8},{:.8},{:.8},{:.8},1000",
                ts.format("%Y-%m-%d %H:%M:%S"),
                price,
                price,
                price,
                price
            )
            .unwrap();
            price *= 1.0 + g;
        }
        f.flush().unwrap();
        f
    }

    /// 生成跨多日的时间网格（>40 bar），每天 4 根 bar。
    fn make_timestamps() -> Vec<NaiveDateTime> {
        use chrono::NaiveDate;
        let days: Vec<u32> = (2..=13).collect(); // 12 天 × 4 bar = 48 bar
        let hm: Vec<(u32, u32)> = vec![(9, 30), (10, 0), (10, 30), (11, 0)];
        let mut ts = Vec::new();
        for &d in &days {
            for &(h, m) in &hm {
                ts.push(
                    NaiveDate::from_ymd_opt(2024, 1, d)
                        .unwrap()
                        .and_hms_opt(h, m, 0)
                        .unwrap(),
                );
            }
        }
        ts
    }

    /// 构建合成 universe：6 标的恒定增长率 g_k 升序。
    /// 返回 (universe_file, bar_files, out_json_file)。
    fn make_6sym_universe(
        timestamps: &[NaiveDateTime],
        growth_rates: &[f64],
        symbols: &[&str],
    ) -> (tempfile::NamedTempFile, Vec<tempfile::NamedTempFile>, tempfile::NamedTempFile) {
        let bar_files: Vec<tempfile::NamedTempFile> = growth_rates
            .iter()
            .map(|&g| write_price_csv(timestamps, 10.0, g))
            .collect();

        let mut univ_f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        writeln!(univ_f, "symbol,primary").unwrap();
        for (sym, bf) in symbols.iter().zip(bar_files.iter()) {
            writeln!(univ_f, "{},{}", sym, bf.path().to_str().unwrap()).unwrap();
        }
        univ_f.flush().unwrap();

        let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        (univ_f, bar_files, out_f)
    }

    /// 合成黄金测试：6 标的恒定增长率 g_k 升序 → 动量因子与未来收益同序。
    #[test]
    fn collect_periods_golden_monotone() {
        let timestamps = make_timestamps();
        let growth_rates = [0.001f64, 0.002, 0.003, 0.004, 0.005, 0.006];
        let symbols: Vec<&str> = vec!["s1", "s2", "s3", "s4", "s5", "s6"];

        let (univ_f, _bar_files, out_f) =
            make_6sym_universe(&timestamps, &growth_rates, &symbols);

        let horizon = 4usize;
        let ladder_expected = stats::decay_ladder(horizon);
        let main_h_idx = ladder_expected
            .iter()
            .position(|&h| h == horizon)
            .expect("horizon must be in ladder");

        let cfg = FactorConfig {
            universe_path: univ_f.path().to_path_buf(),
            factors: vec![FactorSpecItem {
                name: "mom".into(),
                expr: "close/ref(close,4)-1".into(),
            }],
            sample: 4,
            horizon,
            layers: 3,
            warmup: 8,
            window: 20,
            out_path: out_f.path().to_path_buf(),
            html_path: None,
            membership_path: None,
        };

        let (periods, ladder, n_syms) = collect_periods(&cfg).unwrap();

        assert_eq!(n_syms, 6, "universe should have 6 symbols");
        assert!(periods.len() >= 2, "need at least 2 sample periods");

        for period in &periods {
            assert_eq!(
                period.points.len(),
                6,
                "period at {:?} should have 6 points, got {}",
                period.t,
                period.points.len()
            );

            let valid_pairs: Vec<(f64, f64)> = period
                .points
                .iter()
                .filter_map(|pt| {
                    let fv = pt.factors[0]?;
                    let rv = pt.rets[main_h_idx]?;
                    Some((fv, rv))
                })
                .collect();

            if valid_pairs.len() < 2 {
                continue;
            }

            let fvals: Vec<f64> = valid_pairs.iter().map(|(f, _)| *f).collect();
            let rvals: Vec<f64> = valid_pairs.iter().map(|(_, r)| *r).collect();

            let rho = stats::spearman(&fvals, &rvals).unwrap();
            assert!(
                (rho - 1.0).abs() < 1e-6,
                "spearman(factor, ret) should be ≈1.0 for monotone growth rates, got {rho}"
            );

            assert_eq!(
                ladder,
                ladder_expected,
                "ladder should match decay_ladder(horizon)"
            );
        }
    }

    /// 校验：factors 空时返回 Error::Data。
    #[test]
    fn collect_periods_empty_factors_errors() {
        let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        let cfg = FactorConfig {
            universe_path: PathBuf::from("dummy.csv"),
            factors: vec![],
            sample: 4,
            horizon: 4,
            layers: 3,
            warmup: 8,
            window: 20,
            out_path: out_f.path().to_path_buf(),
            html_path: None,
            membership_path: None,
        };
        assert!(collect_periods(&cfg).is_err());
    }

    /// 校验：重名因子返回 Error::Data。
    #[test]
    fn collect_periods_duplicate_factor_name_errors() {
        let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        let cfg = FactorConfig {
            universe_path: PathBuf::from("dummy.csv"),
            factors: vec![
                FactorSpecItem { name: "mom".into(), expr: "close".into() },
                FactorSpecItem { name: "mom".into(), expr: "close".into() },
            ],
            sample: 4,
            horizon: 4,
            layers: 3,
            warmup: 8,
            window: 20,
            out_path: out_f.path().to_path_buf(),
            html_path: None,
            membership_path: None,
        };
        assert!(collect_periods(&cfg).is_err());
    }

    /// 校验：DSL 解析失败时错误信息含因子名。
    #[test]
    fn collect_periods_bad_expr_includes_factor_name() {
        let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        let cfg = FactorConfig {
            universe_path: PathBuf::from("dummy.csv"),
            factors: vec![FactorSpecItem {
                name: "broken".into(),
                expr: "((( bad expr !!!".into(),
            }],
            sample: 4,
            horizon: 4,
            layers: 3,
            warmup: 8,
            window: 20,
            out_path: out_f.path().to_path_buf(),
            html_path: None,
            membership_path: None,
        };
        let err = collect_periods(&cfg).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("broken"),
            "error should mention factor name 'broken', got: {msg}"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Task 3 聚合测试
    // ─────────────────────────────────────────────────────────────────────────

    /// run_factor mom 单因子：rank_ic_mean ≈ 1，单调性 ≈ 1，spread_total > 0，首层 < 末层 ann_return。
    #[test]
    fn run_factor_mom_single() {
        let timestamps = make_timestamps();
        let growth_rates = [0.001f64, 0.002, 0.003, 0.004, 0.005, 0.006];
        let symbols: Vec<&str> = vec!["s1", "s2", "s3", "s4", "s5", "s6"];
        let (univ_f, _bar_files, out_f) =
            make_6sym_universe(&timestamps, &growth_rates, &symbols);

        let cfg = FactorConfig {
            universe_path: univ_f.path().to_path_buf(),
            factors: vec![FactorSpecItem {
                name: "mom".into(),
                expr: "close/ref(close,4)-1".into(),
            }],
            sample: 4,
            horizon: 4,
            layers: 3,
            warmup: 8,
            window: 20,
            out_path: out_f.path().to_path_buf(),
            html_path: None,
            membership_path: None,
        };

        let report = run_factor(&cfg).unwrap();
        assert_eq!(report.factors.len(), 1);
        let fs = &report.factors[0];

        // RankIC should be very close to 1
        let rim = fs.rank_ic_mean.expect("rank_ic_mean should be Some");
        assert!(
            rim > 0.9,
            "rank_ic_mean should be ≈1.0 for momentum factor, got {rim}"
        );

        // ICIR: when all cross-sectional RankICs equal 1.0 (perfectly monotone synthetic
        // fixture), sample_std = 0 so ICIR is correctly None per spec (拒绝假数字).
        // Verify std is either None or ≈ 0 in that case.
        if let Some(std) = fs.rank_ic_std
            && std > 1e-10
        {
            assert!(
                fs.rank_icir.is_some(),
                "rank_icir should be Some when rank_ic_std > 0, std={std}"
            );
        }

        // Layer monotonicity ≈ 1
        let ls = fs.layers.as_ref().expect("layers should be Some");
        let mono = ls.monotonicity.expect("monotonicity should be Some");
        assert!(
            mono > 0.9,
            "monotonicity should be ≈1.0 for momentum factor, got {mono}"
        );

        // Spread total > 0 (top layer outperforms bottom)
        assert!(
            ls.spread_total > 0.0,
            "spread_total should be > 0 for momentum factor, got {}",
            ls.spread_total
        );

        // First layer ann_return < last layer ann_return (where both are Some)
        let first_ann = ls.ann_returns.first().and_then(|v| *v);
        let last_ann = ls.ann_returns.last().and_then(|v| *v);
        if let (Some(fa), Some(la)) = (first_ann, last_ann) {
            assert!(
                fa < la,
                "first layer ann_return ({fa}) should be < last layer ann_return ({la})"
            );
        }

        // JSON was written
        let json_content = std::fs::read_to_string(out_f.path()).unwrap();
        assert!(json_content.contains("rank_ic_mean"), "JSON should contain rank_ic_mean");

        // n_periods > 0
        assert!(fs.n_periods > 0, "n_periods should be > 0");
    }

    /// run_factor rev 反向因子：rank_ic_mean ≈ −1，spread_total < 0，单调性 ≈ −1。
    #[test]
    fn run_factor_rev_reversed() {
        let timestamps = make_timestamps();
        let growth_rates = [0.001f64, 0.002, 0.003, 0.004, 0.005, 0.006];
        let symbols: Vec<&str> = vec!["s1", "s2", "s3", "s4", "s5", "s6"];
        let (univ_f, _bar_files, out_f) =
            make_6sym_universe(&timestamps, &growth_rates, &symbols);

        let cfg = FactorConfig {
            universe_path: univ_f.path().to_path_buf(),
            factors: vec![FactorSpecItem {
                name: "rev".into(),
                expr: "ref(close,4)/close-1".into(),
            }],
            sample: 4,
            horizon: 4,
            layers: 3,
            warmup: 8,
            window: 20,
            out_path: out_f.path().to_path_buf(),
            html_path: None,
            membership_path: None,
        };

        let report = run_factor(&cfg).unwrap();
        let fs = &report.factors[0];

        let rim = fs.rank_ic_mean.expect("rank_ic_mean should be Some for rev factor");
        assert!(
            rim < -0.9,
            "rank_ic_mean should be ≈−1.0 for reverse factor, got {rim}"
        );

        let ls = fs.layers.as_ref().expect("layers should be Some");
        assert!(
            ls.spread_total < 0.0,
            "spread_total should be < 0 for reverse factor, got {}",
            ls.spread_total
        );

        let mono = ls.monotonicity.expect("monotonicity should be Some for rev");
        assert!(
            mono < -0.9,
            "monotonicity should be ≈−1.0 for reverse factor, got {mono}"
        );
    }

    /// 双因子 mom+rev：corr[0][1] ≈ −1，对角 Some(1.0)。
    #[test]
    fn run_factor_dual_corr_neg_one() {
        let timestamps = make_timestamps();
        let growth_rates = [0.001f64, 0.002, 0.003, 0.004, 0.005, 0.006];
        let symbols: Vec<&str> = vec!["s1", "s2", "s3", "s4", "s5", "s6"];
        let (univ_f, _bar_files, out_f) =
            make_6sym_universe(&timestamps, &growth_rates, &symbols);

        let cfg = FactorConfig {
            universe_path: univ_f.path().to_path_buf(),
            factors: vec![
                FactorSpecItem {
                    name: "mom".into(),
                    expr: "close/ref(close,4)-1".into(),
                },
                FactorSpecItem {
                    name: "rev".into(),
                    expr: "ref(close,4)/close-1".into(),
                },
            ],
            sample: 4,
            horizon: 4,
            layers: 3,
            warmup: 8,
            window: 20,
            out_path: out_f.path().to_path_buf(),
            html_path: None,
            membership_path: None,
        };

        let report = run_factor(&cfg).unwrap();

        let corr = report.corr.as_ref().expect("corr should be Some for 2 factors");
        assert_eq!(corr.names, vec!["mom", "rev"]);

        // Diagonal = Some(1.0)
        assert_eq!(corr.values[0][0], Some(1.0));
        assert_eq!(corr.values[1][1], Some(1.0));

        // Off-diagonal ≈ −1
        let c01 = corr.values[0][1].expect("corr[0][1] should be Some");
        assert_relative_eq!(c01, -1.0, epsilon = 0.05);
        let c10 = corr.values[1][0].expect("corr[1][0] should be Some");
        assert_relative_eq!(c10, -1.0, epsilon = 0.05);
    }

    /// 3 标的 universe（< max(5,Q=5)）→ 全期 skipped，IC None，n_periods=0，不 panic。
    #[test]
    fn run_factor_3sym_all_skipped() {
        let timestamps = make_timestamps();
        let growth_rates = [0.001f64, 0.003, 0.006];
        let symbols: Vec<&str> = vec!["s1", "s2", "s3"];
        let (univ_f, _bar_files, out_f) =
            make_6sym_universe(&timestamps, &growth_rates, &symbols);

        let cfg = FactorConfig {
            universe_path: univ_f.path().to_path_buf(),
            factors: vec![FactorSpecItem {
                name: "mom".into(),
                expr: "close/ref(close,4)-1".into(),
            }],
            sample: 4,
            horizon: 4,
            layers: 5, // Q=5 so threshold = max(5,5) = 5; 3 < 5 → all skipped
            warmup: 8,
            window: 20,
            out_path: out_f.path().to_path_buf(),
            html_path: None,
            membership_path: None,
        };

        let report = run_factor(&cfg).unwrap();
        let fs = &report.factors[0];

        assert_eq!(fs.n_periods, 0, "n_periods should be 0 when all periods skipped");
        assert!(fs.ic_mean.is_none(), "ic_mean should be None when all skipped");
        assert!(fs.rank_ic_mean.is_none(), "rank_ic_mean should be None when all skipped");
        // n_skipped > 0
        assert!(fs.n_skipped > 0, "n_skipped should be > 0");
    }

    #[test]
    fn membership_mask_excludes_nonmembers() {
        use crate::data::bar::Bar;
        use chrono::NaiveDate;
        let dir = tempfile::tempdir().unwrap();
        let mk = |base: f64| -> Vec<Bar> {
            (1..=8).map(|d| {
                let t = NaiveDate::from_ymd_opt(2018,1,d).unwrap().and_hms_opt(15,0,0).unwrap();
                Bar { time: t, open: base, high: base+1.0, low: base-1.0, close: base + d as f64, volume: 100.0 }
            }).collect()
        };
        let pa = dir.path().join("A.csv");
        let pb = dir.path().join("B.csv");
        crate::data::reader::write_bars_csv(&mk(10.0), &pa).unwrap();
        crate::data::reader::write_bars_csv(&mk(20.0), &pb).unwrap();
        let uni = dir.path().join("uni.csv");
        std::fs::write(&uni, format!("symbol,primary\nA,{}\nB,{}\n", pa.display(), pb.display())).unwrap();
        let mem = dir.path().join("mem.csv");
        std::fs::write(&mem, "date,symbol\n2018-01-01,A\n").unwrap();

        let cfg = FactorConfig {
            universe_path: uni.clone(),
            factors: vec![FactorSpecItem { name: "px".into(), expr: "close".into() }],
            sample: 1, horizon: 2, layers: 2, warmup: 2, window: 3,
            out_path: dir.path().join("out.json"), html_path: None,
            membership_path: Some(mem),
        };
        let (periods, _ladder, _n) = collect_periods(&cfg).unwrap();
        for p in &periods {
            assert!(p.points.iter().all(|sp| sp.symbol == "A"),
                "period {:?} leaked a non-member", p.t);
        }
        assert!(periods.iter().any(|p| !p.points.is_empty()), "no periods produced");
    }

    #[test]
    fn no_membership_is_frozen_both_symbols() {
        use crate::data::bar::Bar;
        use chrono::NaiveDate;
        let dir = tempfile::tempdir().unwrap();
        let mk = |base: f64| -> Vec<Bar> {
            (1..=8).map(|d| {
                let t = NaiveDate::from_ymd_opt(2018,1,d).unwrap().and_hms_opt(15,0,0).unwrap();
                Bar { time: t, open: base, high: base+1.0, low: base-1.0, close: base + d as f64, volume: 100.0 }
            }).collect()
        };
        let pa = dir.path().join("A.csv");
        let pb = dir.path().join("B.csv");
        crate::data::reader::write_bars_csv(&mk(10.0), &pa).unwrap();
        crate::data::reader::write_bars_csv(&mk(20.0), &pb).unwrap();
        let uni = dir.path().join("uni.csv");
        std::fs::write(&uni, format!("symbol,primary\nA,{}\nB,{}\n", pa.display(), pb.display())).unwrap();
        let cfg = FactorConfig {
            universe_path: uni, factors: vec![FactorSpecItem { name: "px".into(), expr: "close".into() }],
            sample: 1, horizon: 2, layers: 2, warmup: 2, window: 3,
            out_path: dir.path().join("out.json"), html_path: None,
            membership_path: None,
        };
        let (periods, _l, _n) = collect_periods(&cfg).unwrap();
        let any_b = periods.iter().any(|p| p.points.iter().any(|sp| sp.symbol == "B"));
        assert!(any_b, "frozen mode must keep B");
    }
}
