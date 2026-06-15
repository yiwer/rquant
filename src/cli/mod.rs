use crate::backtest::portfolio::{run_portfolio, print_portfolio_summary, PortfolioConfig};
use crate::backtest::runner::{run, BacktestConfig};
use crate::eval::llm::client::OpenAiLlm;
use crate::eval::llm::{LlmConfig, LlmEvaluator};
use crate::factor::{FactorConfig, FactorSpecItem, run_factor, print_factor_summary};
use crate::optimize::{OptimizeConfig, print_optimize_summary, run_optimize};
use crate::signal::{
    SignalSingleConfig, SignalPortfolioConfig,
    run_signal_single, run_signal_portfolio,
    write_paper_state, write_holdings_state,
    print_single_signal, print_portfolio_signal,
};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Returns true iff all three LLM credentials are non-empty.
pub(crate) fn llm_enabled(model: &str, base_url: &str, api_key: &str) -> bool {
    !model.is_empty() && !base_url.is_empty() && !api_key.is_empty()
}

/// 构造 LLM 评估器：三件套齐全→OpenAi，否则提示一次并回退 Disabled。
/// 桌面端桥接层复用(spec §4-2)。
pub fn build_llm(model: String, base_url: String, cache_dir: PathBuf) -> anyhow::Result<LlmEvaluator> {
    let api_key = std::env::var("RQUANT_LLM_API_KEY").unwrap_or_default();
    if llm_enabled(&model, &base_url, &api_key) {
        let cfg = LlmConfig {
            base_url,
            api_key,
            model,
            timeout_secs: 60,
            max_retries: 2,
            cache_dir,
        };
        Ok(LlmEvaluator::OpenAi(OpenAiLlm::new(cfg)?))
    } else {
        eprintln!("[rquant] LLM not configured (need --llm-model, --llm-base-url, env RQUANT_LLM_API_KEY); LLM nodes will take their default branch.");
        Ok(LlmEvaluator::Disabled)
    }
}

/// 解析 --aux NAME=PATH 旗标（重名报错）。
fn parse_aux(specs: &[String]) -> anyhow::Result<Vec<(String, PathBuf)>> {
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    for spec in specs {
        let (n, p) = spec
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("--aux expects NAME=PATH, got '{spec}'"))?;
        if out.iter().any(|(en, _)| en == n) {
            return Err(anyhow::anyhow!("duplicate --aux name '{n}'"));
        }
        out.push((n.to_string(), PathBuf::from(p)));
    }
    Ok(out)
}

// 2026-06 实测：money.finance.sina.com.cn 该服务回 "Service not valid"；quotes.sina.cn 可用
/// 桌面端桥接层复用(spec §4-2)。
pub const SINA_BASE_URL: &str = "https://quotes.sina.cn/cn/api/json_v2.php";

#[derive(Parser)]
#[command(name = "rquant", about = "Fuzzy decision-tree A-share backtester")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum Cmd {
    /// Run a quant backtest over local CSV bars (LLM nodes via OpenAI-standard API if configured)
    Backtest {
        #[arg(long)]
        tree: PathBuf,
        #[arg(long)]
        primary: PathBuf,
        #[arg(long)]
        context: PathBuf,
        #[arg(long)]
        news: Option<PathBuf>,
        #[arg(long, default_value = "report.json")]
        out: PathBuf,
        #[arg(long)]
        traces: Option<PathBuf>,
        #[arg(long, default_value_t = 10.0)]
        cost_bps: f64,
        #[arg(long, default_value_t = 100)]
        warmup: usize,
        #[arg(long, default_value_t = 100)]
        window: usize,
        #[arg(long, default_value_t = 8)]
        concurrency: usize,
        /// Optional A-share holidays file (one YYYY-MM-DD per line) for gap detection
        #[arg(long)]
        holidays: Option<PathBuf>,
        /// Walk-forward folds (>=2 enables fixed-tree rolling-fold stability metrics)
        #[arg(long, default_value_t = 0)]
        folds: usize,
        /// Soft/probabilistic traversal: propagate confidence-weighted leaf distribution
        #[arg(long, default_value_t = false)]
        soft: bool,
        /// Position-state simulation mode (sequential equity; composable with --soft)
        #[arg(long, default_value_t = false)]
        sim: bool,
        #[arg(long, default_value = "")]
        llm_model: String,
        #[arg(long, default_value = "")]
        llm_base_url: String,
        #[arg(long, default_value = ".rquant-cache/llm")]
        llm_cache_dir: PathBuf,
        /// Mount an external series table: --aux name=path.csv (repeatable); DSL: aux.<name>.<column>
        #[arg(long = "aux", value_name = "NAME=PATH")]
        aux: Vec<String>,
    },
    /// Fetch K-line bars from Sina Finance into a local CSV
    Fetch {
        /// Symbol, e.g. sh600000 / sz000001
        #[arg(long)]
        symbol: String,
        /// K-line scale in minutes: 15, 60, 240 (daily)
        #[arg(long)]
        scale: u32,
        /// Output CSV path
        #[arg(long)]
        out: PathBuf,
        /// Max bars to fetch (Sina cap: 1023)
        #[arg(long, default_value_t = 1023)]
        datalen: u32,
        /// Override the Sina endpoint base URL
        #[arg(long, default_value = SINA_BASE_URL)]
        base_url: String,
        /// Price adjustment: none (raw, default) or qfq (forward-adjusted via Tencent daily)
        #[arg(long, default_value = "none")]
        adjust: String,
        /// Deep history: fetch from this date (YYYY-MM-DD) via multi-window stitching (daily qfq only).
        #[arg(long)]
        from: Option<String>,
    },
    /// Generate today's trading signal (single-symbol paper-sim or portfolio target list)
    Signal {
        /// Decision tree YAML file path
        #[arg(long)]
        tree: PathBuf,
        /// Paper state JSON path (read/write; created fresh if absent)
        #[arg(long)]
        state: PathBuf,
        /// Primary K-line CSV path (single mode)
        #[arg(long)]
        primary: Option<PathBuf>,
        /// Context bars CSV path (single mode; defaults to --primary if omitted)
        #[arg(long)]
        context: Option<PathBuf>,
        /// News CSV path (optional; for LLM nodes)
        #[arg(long)]
        news: Option<PathBuf>,
        /// Universe CSV path (portfolio mode; mutually exclusive with --primary)
        #[arg(long)]
        universe: Option<PathBuf>,
        /// Number of top symbols to hold (portfolio mode)
        #[arg(long, default_value_t = 5)]
        top: usize,
        /// Optional: refresh --primary from network first (single mode only)
        #[arg(long)]
        fetch: Option<String>,
        /// K-line scale in minutes (only used with --fetch): 15, 60, 240 (daily)
        #[arg(long, default_value_t = 60)]
        scale: u32,
        /// Max bars to fetch (only used with --fetch; Sina cap: 1023)
        #[arg(long, default_value_t = 1023)]
        datalen: u32,
        /// Price adjustment for --fetch: none (raw) or qfq (forward-adjusted)
        #[arg(long, default_value = "none")]
        adjust: String,
        #[arg(long, default_value_t = false)]
        soft: bool,
        /// Commit signal to state file (dry-run if omitted)
        #[arg(long, default_value_t = false)]
        commit: bool,
        /// Optional: write signal JSON to this file
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long, default_value_t = 100)]
        warmup: usize,
        #[arg(long, default_value_t = 100)]
        window: usize,
        #[arg(long, default_value_t = 10.0)]
        cost_bps: f64,
        #[arg(long = "aux", value_name = "NAME=PATH")]
        aux: Vec<String>,
        #[arg(long, default_value = "")]
        llm_model: String,
        #[arg(long, default_value = "")]
        llm_base_url: String,
        #[arg(long, default_value = ".rquant-cache/llm")]
        llm_cache_dir: PathBuf,
    },
    /// Render a report.json (+ optional traces/primary) into a self-contained HTML report
    Report {
        #[arg(long)]
        report: PathBuf,
        #[arg(long, default_value = "report.html")]
        out: PathBuf,
        #[arg(long)]
        traces: Option<PathBuf>,
        #[arg(long)]
        primary: Option<PathBuf>,
        /// Render a soft-mode report (soft_report.json + soft_traces.jsonl); no --primary needed
        #[arg(long, default_value_t = false)]
        soft: bool,
        /// Render a sim_report.json (use with --traces for nav/pos curves)
        #[arg(long, default_value_t = false)]
        sim: bool,
        /// Render a portfolio.json (self-contained)
        #[arg(long, default_value_t = false)]
        portfolio: bool,
    },
    /// Cross-sectional factor workbench: IC/RankIC, decay, quantile layers, correlation
    Factor {
        #[arg(long)]
        universe: PathBuf,
        /// Repeatable: --factor "name=DSL expr"
        #[arg(long = "factor", value_name = "NAME=EXPR")]
        factor: Vec<String>,
        #[arg(long, default_value_t = 16)]
        sample: usize,
        #[arg(long, default_value_t = 16)]
        horizon: usize,
        #[arg(long, default_value_t = 5)]
        layers: usize,
        #[arg(long, default_value_t = 100)]
        warmup: usize,
        #[arg(long, default_value_t = 100)]
        window: usize,
        #[arg(long, default_value = "factor_report.json")]
        out: PathBuf,
        #[arg(long)]
        html: Option<PathBuf>,
    },
    /// 日线选股器：多树集成 → 优质+投机形态标注（as-of），或历史回测验证（--backtest）。
    Screen {
        #[arg(long)]
        universe: PathBuf,
        #[arg(long, default_value = "examples/screen_v1.yaml")]
        config: PathBuf,
        /// 历史回测模式（回放集成、出净值/归因/regime/质量分层）
        #[arg(long, default_value_t = false)]
        backtest: bool,
        /// as-of 日期（选股模式；默认最新 K）YYYY-MM-DD
        #[arg(long)]
        as_of: Option<String>,
        /// 回测起始日 YYYY-MM-DD
        #[arg(long)]
        from: Option<String>,
        /// 回测结束日 YYYY-MM-DD
        #[arg(long)]
        to: Option<String>,
        #[arg(long)]
        top: Option<usize>,
        #[arg(long, default_value_t = 5)]
        rebalance: usize,
        #[arg(long, default_value_t = 260)]
        warmup: usize,
        #[arg(long, default_value_t = 260)]
        window: usize,
        #[arg(long, default_value_t = 10.0)]
        cost_bps: f64,
        #[arg(long, default_value_t = false)]
        soft: bool,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long, default_value = "")]
        llm_model: String,
        #[arg(long, default_value = "")]
        llm_base_url: String,
        #[arg(long, default_value = ".rquant-cache/llm")]
        llm_cache_dir: PathBuf,
    },
    /// Walk-forward parameter optimization (grid x anchored-expanding IS -> OS)
    Optimize {
        #[arg(long)]
        tree: PathBuf,
        #[arg(long)]
        primary: PathBuf,
        #[arg(long)]
        context: PathBuf,
        #[arg(long)]
        news: Option<PathBuf>,
        /// Repeatable: --grid "name=start:stop:step" or "name=v1,v2,..."
        #[arg(long = "grid", value_name = "NAME=VALUES")]
        grid: Vec<String>,
        #[arg(long, default_value_t = 5)]
        folds: usize,
        #[arg(long, default_value_t = false)]
        sim: bool,
        #[arg(long, default_value_t = false)]
        soft: bool,
        #[arg(long, default_value_t = 500)]
        max_combos: usize,
        #[arg(long, default_value_t = 100)]
        warmup: usize,
        #[arg(long, default_value_t = 100)]
        window: usize,
        #[arg(long, default_value_t = 10.0)]
        cost_bps: f64,
        #[arg(long = "aux", value_name = "NAME=PATH")]
        aux: Vec<String>,
        #[arg(long, default_value = "optimize_report.json")]
        out: PathBuf,
        #[arg(long, default_value = "")]
        llm_model: String,
        #[arg(long, default_value = "")]
        llm_base_url: String,
        #[arg(long, default_value = ".rquant-cache/llm")]
        llm_cache_dir: PathBuf,
        /// Boundary-escape for gate-4 interior optimum: max extension steps per axis (0 = off)
        #[arg(long, default_value_t = 0)]
        auto_extend: usize,
    },
    /// Cross-sectional portfolio: run one tree across a universe, hold top-N equal-weight
    Portfolio {
        #[arg(long)]
        tree: PathBuf,
        #[arg(long)]
        universe: PathBuf,
        #[arg(long, default_value_t = 5)]
        top: usize,
        #[arg(long, default_value_t = 16)]
        rebalance: usize,
        #[arg(long, default_value_t = 100)]
        warmup: usize,
        #[arg(long, default_value_t = 100)]
        window: usize,
        #[arg(long, default_value_t = 10.0)]
        cost_bps: f64,
        #[arg(long, default_value_t = false)]
        soft: bool,
        #[arg(long = "aux", value_name = "NAME=PATH")]
        aux: Vec<String>,
        #[arg(long, default_value = "portfolio.json")]
        out: PathBuf,
        #[arg(long)]
        traces: Option<PathBuf>,
        #[arg(long, default_value = "")]
        llm_model: String,
        #[arg(long, default_value = "")]
        llm_base_url: String,
        #[arg(long, default_value = ".rquant-cache/llm")]
        llm_cache_dir: PathBuf,
    },
    /// Apply the 5-gate WFO certification to N per-symbol optimize reports.
    Eval {
        /// Repeatable: one optimize JSON per symbol (a strategy's universe).
        #[arg(long = "reports", value_name = "PATH", required = true)]
        reports: Vec<PathBuf>,
        /// Strategy name for the verdict (default: derived from first symbol).
        #[arg(long, default_value = "")]
        name: String,
        /// Write Verdict JSON here.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Validate fetched CSV data quality (monotonic time, gross jumps, gaps, coverage).
    ValidateData {
        /// Repeatable: one CSV per call.
        #[arg(long = "csv", value_name = "PATH", required = true)]
        csv: Vec<PathBuf>,
        /// Optional holidays file (YYYY-MM-DD per line) for accurate gap counting.
        #[arg(long)]
        holidays: Option<PathBuf>,
        /// Suspicious-jump threshold on |daily return| (default 0.21 = beyond ChiNext ±20%).
        #[arg(long, default_value_t = 0.21)]
        jump: f64,
    },
}

/// Fetch K-line bars from Sina and write to a CSV file.
/// Returns the number of bars written. Prints no output itself (caller prints).
/// 桌面端桥接层复用(spec §4-2)。
pub async fn run_fetch_to_csv(
    symbol: &str,
    scale: u32,
    datalen: u32,
    base_url: &str,
    adjust: &str,
    out: &std::path::Path,
    from: Option<chrono::NaiveDate>,
) -> anyhow::Result<usize> {
    if adjust != "none" && adjust != "qfq" {
        return Err(anyhow::anyhow!("--adjust must be 'none' or 'qfq'"));
    }
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let bars = if adjust == "qfq" {
        use crate::data::tencent::{fetch_tencent_daily, fetch_tencent_daily_deep, TENCENT_FQKLINE_BASE};
        if scale == 240 {
            let raw = match from {
                Some(earliest) => fetch_tencent_daily_deep(&http, TENCENT_FQKLINE_BASE, symbol, earliest, "qfq").await?,
                None => fetch_tencent_daily(&http, TENCENT_FQKLINE_BASE, symbol, datalen, "qfq").await?,
            };
            let (clean, trimmed) = crate::data::quality::trim_incoherent_leading(&raw, 0.5);
            if trimmed > 0 {
                eprintln!("[rquant] trimmed {trimmed} incoherent leading qfq bars for {symbol}");
            }
            clean
        } else {
            // 三源合成：因子表天数 = 分钟 bar 覆盖天数 + 30 裕量（240/scale = bars/日）
            let daily_len = (datalen * scale / 240 + 30).min(1023);
            let raw_min = crate::data::sina::fetch_sina_klines(&http, base_url, symbol, scale, datalen, 2).await?;
            let raw_d = fetch_tencent_daily(&http, TENCENT_FQKLINE_BASE, symbol, daily_len, "").await?;
            let qfq_d = fetch_tencent_daily(&http, TENCENT_FQKLINE_BASE, symbol, daily_len, "qfq").await?;
            let factors = crate::data::adjust::adjust_factors(&raw_d, &qfq_d)?;
            eprintln!("[rquant] qfq synthesis: {} factor days x {} intraday bars", factors.len(), raw_min.len());
            crate::data::adjust::apply_factors(&raw_min, &factors)?
        }
    } else {
        crate::data::sina::fetch_sina_klines(&http, base_url, symbol, scale, datalen, 2).await?
    };
    crate::data::reader::write_bars_csv(&bars, out)?;
    Ok(bars.len())
}

#[tokio::main]
pub async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Backtest {
            tree, primary, context, news, out, traces, cost_bps, warmup, window, concurrency,
            holidays, folds, soft, sim, llm_model, llm_base_url, llm_cache_dir, aux,
        } => {
            let llm = build_llm(llm_model, llm_base_url, llm_cache_dir)?;
            let aux_paths = parse_aux(&aux)?;
            let cfg = BacktestConfig {
                tree_path: tree, primary_path: primary, context_path: context, news_path: news,
                out_path: out, traces_path: traces, cost_bps, warmup, window, concurrency,
                holidays_path: holidays, folds, aux_paths, decision_traces_path: None,
            };
            if sim {
                if folds >= 2 {
                    eprintln!("[rquant] note: --folds is ignored in --sim mode");
                }
                let report = crate::backtest::sim::run_sim(&cfg, &llm, soft).await?;
                crate::backtest::sim::print_sim_summary(&report);
            } else if soft {
                let report = crate::backtest::soft::run_soft(&cfg, &llm).await?;
                crate::report::print_soft_summary(&report);
            } else {
                let report = run(&cfg, &llm).await?;
                crate::report::print_summary(&report);
            }
        }
        Cmd::Optimize {
            tree, primary, context, news, grid, folds, sim, soft, max_combos,
            warmup, window, cost_bps, aux, out, llm_model, llm_base_url, llm_cache_dir,
            auto_extend,
        } => {
            if sim && soft {
                return Err(anyhow::anyhow!(
                    "--sim and --soft are mutually exclusive for optimize (sim target is undefined in soft-score mode)"
                ));
            }
            let llm = build_llm(llm_model, llm_base_url, llm_cache_dir)?;
            let aux_paths = parse_aux(&aux)?;
            if grid.is_empty() {
                return Err(anyhow::anyhow!(
                    "--grid: at least one grid axis is required (use --grid 'name=start:stop:step')"
                ));
            }
            let ocfg = OptimizeConfig {
                tree_path: tree,
                primary_path: primary,
                context_path: context,
                news_path: news,
                aux_paths,
                window,
                warmup,
                cost_bps,
                folds,
                sim,
                soft,
                grids: grid,
                max_combos,
                auto_extend,
                out_path: out,
            };
            let report = run_optimize(&ocfg, &llm).await?;
            print_optimize_summary(&report);
        }
        Cmd::Factor { universe, factor, sample, horizon, layers, warmup, window, out, html } => {
            if factor.is_empty() {
                return Err(anyhow::anyhow!("--factor: at least one factor expression is required (use --factor 'name=expr')"));
            }
            let mut factors: Vec<FactorSpecItem> = Vec::new();
            for spec in &factor {
                let eq_pos = spec.find('=').ok_or_else(|| {
                    anyhow::anyhow!("--factor expects 'NAME=EXPR', got '{spec}' (missing '=')")
                })?;
                let name = &spec[..eq_pos];
                let expr = &spec[eq_pos + 1..];
                if name.is_empty() {
                    return Err(anyhow::anyhow!("--factor: factor name must not be empty in '{spec}'"));
                }
                if expr.is_empty() {
                    return Err(anyhow::anyhow!("--factor: factor expression must not be empty in '{spec}'"));
                }
                if factors.iter().any(|f| f.name == name) {
                    return Err(anyhow::anyhow!("--factor: duplicate factor name '{name}'"));
                }
                factors.push(FactorSpecItem { name: name.to_string(), expr: expr.to_string() });
            }
            let cfg = FactorConfig {
                universe_path: universe,
                factors,
                sample,
                horizon,
                layers,
                warmup,
                window,
                out_path: out,
                html_path: html.clone(),
            };
            let report = run_factor(&cfg)?;
            print_factor_summary(&report);
            if let Some(html_path) = html {
                let html_str = crate::report::viz::render_factor_html(&report);
                std::fs::write(&html_path, &html_str)?;
                println!("wrote factor HTML report to {}", html_path.display());
            }
        }
        Cmd::Fetch { symbol, scale, out, datalen, base_url, adjust, from } => {
            let from_date = from
                .map(|s| chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d"))
                .transpose()
                .map_err(|e| anyhow::anyhow!("--from: invalid date: {e}"))?;
            let n = run_fetch_to_csv(&symbol, scale, datalen, &base_url, &adjust, &out, from_date).await?;
            println!("wrote {} bars to {}", n, out.display());
        }
        Cmd::Signal {
            tree, state, primary, context, news, universe, top,
            fetch, scale, datalen, adjust, soft, commit, out,
            warmup, window, cost_bps, aux, llm_model, llm_base_url, llm_cache_dir,
        } => {
            // ── mode mutex check ──────────────────────────────────────────────
            if primary.is_some() == universe.is_some() {
                return Err(anyhow::anyhow!(
                    "exactly one of --primary or --universe is required"
                ));
            }
            if fetch.is_some() && primary.is_none() {
                return Err(anyhow::anyhow!(
                    "--fetch requires --primary (single-symbol mode only)"
                ));
            }

            // ── optional pre-fetch ─────────────────────────────────────────────
            if let Some(ref sym) = fetch {
                let primary_path = primary.as_ref().unwrap();
                let n = run_fetch_to_csv(sym, scale, datalen, SINA_BASE_URL, &adjust, primary_path, None).await?;
                println!("fetched {} bars for {} → {}", n, sym, primary_path.display());
            }

            // ── LLM setup ─────────────────────────────────────────────────────
            let llm = build_llm(llm_model, llm_base_url, llm_cache_dir)?;

            // ── aux parse ─────────────────────────────────────────────────────
            let aux_paths = parse_aux(&aux)?;

            if let Some(primary_path) = primary {
                // ── single-symbol mode ────────────────────────────────────────
                let context_path = context.unwrap_or_else(|| primary_path.clone());
                let cfg = SignalSingleConfig {
                    tree_path: tree,
                    primary_path,
                    context_path,
                    news_path: news,
                    aux_paths,
                    window,
                    warmup,
                    cost_bps,
                    soft,
                    state_path: state.clone(),
                };
                let (sig, new_state) = run_signal_single(&cfg, &llm).await?;
                print_single_signal(&sig);

                if let Some(ref out_path) = out {
                    let json = serde_json::to_string_pretty(&sig)?;
                    std::fs::write(out_path, &json)?;
                    println!("wrote signal JSON to {}", out_path.display());
                }

                if commit {
                    write_paper_state(&state, &new_state)?;
                    println!("committed state to {}", state.display());
                } else {
                    println!("[DRY RUN] 未落盘 state；加 --commit 提交");
                }
            } else {
                // ── portfolio mode ────────────────────────────────────────────
                let universe_path = universe.expect("invariant: portfolio mode requires --universe (checked above)");
                let cfg = SignalPortfolioConfig {
                    tree_path: tree,
                    universe_path,
                    top,
                    window,
                    warmup,
                    cost_bps,
                    soft,
                    aux_paths,
                    state_path: state.clone(),
                };
                let (sig, new_state) = run_signal_portfolio(&cfg, &llm).await?;
                print_portfolio_signal(&sig);

                if let Some(ref out_path) = out {
                    let json = serde_json::to_string_pretty(&sig)?;
                    std::fs::write(out_path, &json)?;
                    println!("wrote signal JSON to {}", out_path.display());
                }

                if commit {
                    write_holdings_state(&state, &new_state)?;
                    println!("committed state to {}", state.display());
                } else {
                    println!("[DRY RUN] 未落盘 state；加 --commit 提交");
                }
            }
        }
        Cmd::Report { report, out, traces, primary, soft, sim, portfolio } => {
            let picked = [soft, sim, portfolio].iter().filter(|b| **b).count();
            if picked > 1 {
                return Err(anyhow::anyhow!("--soft / --sim / --portfolio are mutually exclusive"));
            }
            let mode = if soft {
                crate::report::ReportMode::Soft
            } else if sim {
                crate::report::ReportMode::Sim
            } else if portfolio {
                crate::report::ReportMode::Portfolio
            } else {
                crate::report::ReportMode::Hard
            };
            crate::report::render_report_files(&report, &out, traces.as_deref(), primary.as_deref(), mode)?;
        }
        Cmd::Portfolio {
            tree, universe, top, rebalance, warmup, window, cost_bps, soft, aux, out, traces,
            llm_model, llm_base_url, llm_cache_dir,
        } => {
            let llm = build_llm(llm_model, llm_base_url, llm_cache_dir)?;
            let aux_paths = parse_aux(&aux)?;
            let pcfg = PortfolioConfig {
                tree_path: tree,
                universe_path: universe,
                top,
                rebalance,
                warmup,
                window,
                cost_bps,
                soft,
                aux_paths,
                out_path: out,
                traces_path: traces,
            };
            let report = run_portfolio(&pcfg, &llm).await?;
            print_portfolio_summary(&report);
        }
        Cmd::Screen {
            universe, config, backtest, as_of, from, to, top, rebalance,
            warmup, window, cost_bps, soft, out, llm_model, llm_base_url, llm_cache_dir,
        } => {
            let llm = build_llm(llm_model, llm_base_url, llm_cache_dir)?;
            let parse_date = |o: Option<String>| -> crate::Result<Option<chrono::NaiveDate>> {
                match o {
                    None => Ok(None),
                    Some(s) => chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d")
                        .map(Some)
                        .map_err(|e| crate::Error::Data(format!("bad date '{s}': {e}"))),
                }
            };
            if backtest {
                let bcfg = crate::screen::backtest::ScreenBacktestConfig {
                    config_path: config,
                    universe_path: universe,
                    from: parse_date(from)?,
                    to: parse_date(to)?,
                    rebalance,
                    top,
                    warmup,
                    window,
                    cost_bps,
                    soft,
                    out_path: out,
                };
                let report = crate::screen::backtest::run_screen_backtest(&bcfg, &llm).await?;
                crate::screen::backtest::print_screen_backtest(&report);
            } else {
                let rcfg = crate::screen::ScreenRunConfig {
                    config_path: config,
                    universe_path: universe,
                    as_of: parse_date(as_of)?,
                    top,
                    window,
                    out_path: out,
                };
                let result = crate::screen::run_screen(&rcfg, &llm).await?;
                crate::screen::print_screen(&result);
            }
        }
        Cmd::Eval { reports, name, out } => {
            if reports.is_empty() {
                return Err(anyhow::anyhow!("--reports: at least one optimize report is required"));
            }
            let mut loaded: Vec<(String, crate::optimize::OptimizeReport)> = Vec::new();
            for p in &reports {
                let txt = std::fs::read_to_string(p)
                    .map_err(|e| anyhow::anyhow!("read {}: {e}", p.display()))?;
                let r: crate::optimize::OptimizeReport = serde_json::from_str(&txt)
                    .map_err(|e| anyhow::anyhow!("parse {}: {e}", p.display()))?;
                let symbol = if r.primary.is_empty() {
                    p.file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| p.display().to_string())
                } else {
                    r.primary.clone()
                };
                loaded.push((symbol, r));
            }
            let strategy = if name.is_empty() {
                loaded.first().map(|(s, _)| s.clone()).unwrap_or_default()
            } else {
                name
            };
            let verdict = crate::verdict::certify(
                &loaded,
                &strategy,
                &crate::verdict::GateThresholds::default(),
            );
            print_verdict(&verdict);
            if let Some(op) = out {
                std::fs::write(&op, serde_json::to_string_pretty(&verdict)?)?;
            }
            if !verdict.certified {
                std::process::exit(1);
            }
        }
        Cmd::ValidateData { csv, holidays, jump } => {
            if csv.is_empty() {
                return Err(anyhow::anyhow!("--csv: at least one CSV path is required"));
            }
            let calendar = match &holidays {
                Some(hp) => crate::data::calendar::AShareCalendar::new(
                    crate::data::calendar::read_holidays(hp)?,
                ),
                None => crate::data::calendar::AShareCalendar::new(std::collections::HashSet::new()),
            };
            let mut any_fail = false;
            for path in &csv {
                let bars = crate::data::reader::read_bars_csv(path)
                    .map_err(|e| anyhow::anyhow!("read {}: {e}", path.display()))?;
                let q = crate::data::quality::analyze(&bars, &calendar, jump);
                print_quality(path, &q, holidays.is_none());
                if !q.strictly_increasing || !q.suspicious_jumps.is_empty() {
                    any_fail = true;
                }
            }
            if any_fail {
                std::process::exit(1);
            }
        }
    }
    Ok(())
}

fn print_quality(path: &std::path::Path, q: &crate::data::quality::QualityReport, no_holidays: bool) {
    println!("=== {} ===", path.display());
    println!("  bars       : {}", q.n_bars);
    println!("  coverage   : {} .. {}", q.first, q.last);
    println!("  monotonic  : {}", q.strictly_increasing);
    println!("  max |ret|  : {:.4}", q.max_abs_daily_return);
    println!("  jumps>thr  : {}", q.suspicious_jumps.len());
    for (t, r) in &q.suspicious_jumps {
        println!("    - {t}  ret={r:+.4}");
    }
    let gap_note = if no_holidays { " (incl. market holidays; pass --holidays for accuracy)" } else { "" };
    println!("  gaps       : {}{}", q.calendar_gaps, gap_note);
}

fn print_verdict(v: &crate::verdict::Verdict) {
    use crate::verdict::GateStatus;
    println!(
        "=== WFO 5-Gate Verdict: {} ({} symbols) ===",
        v.strategy, v.n_symbols
    );
    println!("{:<16} {:<14} {:>8} {:>8}  note", "gate", "status", "value", "thresh");
    for g in &v.gates {
        let st = match g.status {
            GateStatus::Pass => "PASS",
            GateStatus::Fail => "FAIL",
            GateStatus::Indeterminate => "INDET",
        };
        println!(
            "{:<16} {:<14} {:>8.3} {:>8.3}  {}",
            g.gate, st, g.value, g.threshold, g.note
        );
    }
    if v.certified {
        println!("RESULT: CERTIFIED");
    } else {
        println!(
            "RESULT: NOT CERTIFIED  failed: [{}]",
            v.failed_gates.join(", ")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::llm_enabled;

    #[test]
    fn llm_enabled_all_set_is_true() {
        assert!(llm_enabled("gpt-4", "https://api.openai.com", "sk-abc"));
    }

    #[test]
    fn llm_enabled_empty_model_is_false() {
        assert!(!llm_enabled("", "https://api.openai.com", "sk-abc"));
    }

    #[test]
    fn llm_enabled_empty_base_url_is_false() {
        assert!(!llm_enabled("gpt-4", "", "sk-abc"));
    }

    #[test]
    fn llm_enabled_empty_api_key_is_false() {
        assert!(!llm_enabled("gpt-4", "https://api.openai.com", ""));
    }
}
