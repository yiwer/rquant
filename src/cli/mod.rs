use crate::backtest::runner::{run, BacktestConfig};
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
    /// Run a quant backtest over local CSV bars
    Backtest {
        #[arg(long)]
        tree: PathBuf,
        #[arg(long)]
        primary: PathBuf,
        #[arg(long)]
        context: PathBuf,
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
    },
}

pub fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Backtest { tree, primary, context, out, traces, cost_bps, warmup, window } => {
            let cfg = BacktestConfig {
                tree_path: tree,
                primary_path: primary,
                context_path: context,
                out_path: out,
                traces_path: traces,
                cost_bps,
                warmup,
                window,
            };
            let report = run(&cfg)?;
            crate::report::print_summary(&report);
        }
    }
    Ok(())
}
