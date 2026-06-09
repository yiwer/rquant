use crate::backtest::runner::{run, BacktestConfig};
use crate::eval::llm::client::OpenAiLlm;
use crate::eval::llm::{LlmConfig, LlmEvaluator};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "rquant", about = "Fuzzy decision-tree A-share backtester")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
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
        #[arg(long, default_value = "")]
        llm_model: String,
        #[arg(long, default_value = "")]
        llm_base_url: String,
        #[arg(long, default_value = ".rquant-cache/llm")]
        llm_cache_dir: PathBuf,
    },
}

#[tokio::main]
pub async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Backtest {
            tree, primary, context, news, out, traces, cost_bps, warmup, window, concurrency,
            llm_model, llm_base_url, llm_cache_dir,
        } => {
            let api_key = std::env::var("RQUANT_LLM_API_KEY").unwrap_or_default();
            let llm = if !llm_model.is_empty() && !llm_base_url.is_empty() && !api_key.is_empty() {
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
            };
            let report = run(&cfg, &llm).await?;
            crate::report::print_summary(&report);
        }
    }
    Ok(())
}
