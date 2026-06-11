pub mod stats;

use crate::backtest::costs::CostModel;
use crate::backtest::forward_return::forward_return;
use crate::backtest::portfolio::build_timeline;
use crate::data::universe::read_universe_csv;
use crate::dsl::parser::parse_str;
use crate::dsl::eval::eval_scalar;
use crate::features::context::build_context;
use crate::tree::schema::Stance;
use crate::{Error, Result};
use chrono::NaiveDateTime;
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
}

/// 一个采样期的原始观测：每标的（因子值按因子序、收益按阶梯序对齐）。
#[derive(Debug)]
#[allow(dead_code)] // consumed in T3
pub(crate) struct SymbolPoint {
    pub symbol: String,
    pub factors: Vec<Option<f64>>, // 非有限 → None
    pub rets: Vec<Option<f64>>,    // forward_return gross；尾部不足 → None
}

#[derive(Debug)]
#[allow(dead_code)] // consumed in T3
pub(crate) struct PeriodData {
    pub t: NaiveDateTime,
    pub points: Vec<SymbolPoint>,
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
#[allow(dead_code)] // consumed in T3
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

    let mut primaries: Vec<Vec<crate::data::bar::Bar>> = Vec::with_capacity(n_symbols);
    let mut contexts: Vec<Vec<crate::data::bar::Bar>> = Vec::with_capacity(n_symbols);
    for entry in &universe {
        primaries.push(crate::data::reader::read_bars_csv(&entry.primary)?);
        contexts.push(crate::data::reader::read_bars_csv(&entry.context)?);
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
        let mut points: Vec<SymbolPoint> = Vec::with_capacity(n_symbols);

        for (sym_idx, entry) in universe.iter().enumerate() {
            let bars = &primaries[sym_idx];
            let ctx_bars = &contexts[sym_idx];

            // 仅恰有 bar 在 t 的标的参与（is_fresh 的索引版）
            let bar_i = match bars.binary_search_by_key(&t, |b| b.time) {
                Ok(i) => i,
                Err(_) => continue, // 该标的在此时刻无 bar
            };

            // 构建 context（news 空、aux 空）
            let ctx = build_context(
                bars,
                ctx_bars,
                &[],
                &empty_aux,
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
// 测试
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
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

    /// 合成黄金测试：6 标的恒定增长率 g_k 升序 → 动量因子与未来收益同序。
    ///
    /// - 标的 k 价格 p_t = 10·(1+g_k)^t，g_k ∈ {0.001, 0.002, 0.003, 0.004, 0.005, 0.006}
    /// - 同一时间网格（跨多日，>40 bar）
    /// - factor "mom=close/ref(close,4)-1"
    /// - 断言：每个采样期有 6 个有效 points，且
    ///   spearman(factor_vals, main_H_rets) ≈ 1.0
    #[test]
    fn collect_periods_golden_monotone() {
        let timestamps = make_timestamps();
        let growth_rates = [0.001f64, 0.002, 0.003, 0.004, 0.005, 0.006];
        let symbols: Vec<&str> = vec!["s1", "s2", "s3", "s4", "s5", "s6"];

        // 生成各标的价格 CSV
        let bar_files: Vec<tempfile::NamedTempFile> = growth_rates
            .iter()
            .map(|&g| write_price_csv(&timestamps, 10.0, g))
            .collect();

        // universe CSV
        let mut univ_f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        writeln!(univ_f, "symbol,primary").unwrap();
        for (sym, bf) in symbols.iter().zip(bar_files.iter()) {
            writeln!(univ_f, "{},{}", sym, bf.path().to_str().unwrap()).unwrap();
        }
        univ_f.flush().unwrap();

        // factor config: horizon=4, sample=4, warmup=8, window=20
        // decay_ladder(4) = [1, 2, 4, 8, 16]
        // main H=4 is at index 2 in ladder
        let horizon = 4usize;
        let ladder_expected = stats::decay_ladder(horizon);
        let main_h_idx = ladder_expected
            .iter()
            .position(|&h| h == horizon)
            .expect("horizon must be in ladder");

        let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();

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
        };

        let (periods, ladder, n_syms) = collect_periods(&cfg).unwrap();

        assert_eq!(n_syms, 6, "universe should have 6 symbols");
        assert!(periods.len() >= 2, "need at least 2 sample periods");

        for period in &periods {
            // 每期应有 6 个有效 points（所有标的均 fresh）
            assert_eq!(
                period.points.len(),
                6,
                "period at {:?} should have 6 points, got {}",
                period.t,
                period.points.len()
            );

            // 收集 (factor_val, main_H_ret) 有效对
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
                // 尾部可能因越界而得不到 ret，跳过该期
                continue;
            }

            let fvals: Vec<f64> = valid_pairs.iter().map(|(f, _)| *f).collect();
            let rvals: Vec<f64> = valid_pairs.iter().map(|(_, r)| *r).collect();

            let rho = stats::spearman(&fvals, &rvals).unwrap();
            assert!(
                (rho - 1.0).abs() < 1e-6,
                "spearman(factor, ret) should be ≈1.0 for monotone growth rates, got {rho}"
            );

            // Verify ladder is correct
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
        // universe path won't even be opened since validation fails first
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
        };
        let err = collect_periods(&cfg).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("broken"),
            "error should mention factor name 'broken', got: {msg}"
        );
    }
}
