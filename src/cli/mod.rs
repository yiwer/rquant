use crate::backtest::runner::{run, BacktestConfig};
use crate::eval::llm::client::OpenAiLlm;
use crate::eval::llm::{LlmConfig, LlmEvaluator};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Returns true iff all three LLM credentials are non-empty.
pub(crate) fn llm_enabled(model: &str, base_url: &str, api_key: &str) -> bool {
    !model.is_empty() && !base_url.is_empty() && !api_key.is_empty()
}

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
        #[arg(long, default_value = "")]
        llm_model: String,
        #[arg(long, default_value = "")]
        llm_base_url: String,
        #[arg(long, default_value = ".rquant-cache/llm")]
        llm_cache_dir: PathBuf,
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
        // 2026-06 实测：money.finance.sina.com.cn 该服务回 "Service not valid"；quotes.sina.cn 可用
        #[arg(long, default_value = "https://quotes.sina.cn/cn/api/json_v2.php")]
        base_url: String,
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
    },
}

#[tokio::main]
pub async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Backtest {
            tree, primary, context, news, out, traces, cost_bps, warmup, window, concurrency,
            holidays, folds, soft, llm_model, llm_base_url, llm_cache_dir,
        } => {
            let api_key = std::env::var("RQUANT_LLM_API_KEY").unwrap_or_default();
            let llm = if llm_enabled(&llm_model, &llm_base_url, &api_key) {
                let cfg = LlmConfig {
                    base_url: llm_base_url,
                    api_key,
                    model: llm_model,
                    timeout_secs: 60,
                    max_retries: 2,
                    cache_dir: llm_cache_dir,
                };
                LlmEvaluator::OpenAi(OpenAiLlm::new(cfg)?)
            } else {
                eprintln!("[rquant] LLM not configured (need --llm-model, --llm-base-url, env RQUANT_LLM_API_KEY); LLM nodes will take their default branch.");
                LlmEvaluator::Disabled
            };
            let cfg = BacktestConfig {
                tree_path: tree, primary_path: primary, context_path: context, news_path: news,
                out_path: out, traces_path: traces, cost_bps, warmup, window, concurrency,
                holidays_path: holidays, folds,
            };
            if soft {
                let report = crate::backtest::soft::run_soft(&cfg, &llm).await?;
                crate::report::print_soft_summary(&report);
            } else {
                let report = run(&cfg, &llm).await?;
                crate::report::print_summary(&report);
            }
        }
        Cmd::Fetch { symbol, scale, out, datalen, base_url } => {
            let http = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()?;
            let bars = crate::data::sina::fetch_sina_klines(&http, &base_url, &symbol, scale, datalen, 2).await?;
            crate::data::reader::write_bars_csv(&bars, &out)?;
            println!("wrote {} bars to {}", bars.len(), out.display());
        }
        Cmd::Report { report, out, traces, primary, soft } => {
            crate::report::render_report_files(&report, &out, traces.as_deref(), primary.as_deref(), soft)?;
        }
    }
    Ok(())
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
