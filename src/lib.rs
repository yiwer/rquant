pub mod data;
pub mod features;
pub mod dsl;
pub mod tree;
pub mod eval;
pub mod engine;
pub mod backtest;
pub mod report;
pub mod cli;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("data error: {0}")]
    Data(String),
    #[error("dsl error: {0}")]
    Dsl(String),
    #[error("tree error: {0}")]
    Tree(String),
    #[error("eval error: {0}")]
    Eval(String),
    #[error("engine error: {0}")]
    Engine(String),
    #[error("backtest error: {0}")]
    Backtest(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("csv error: {0}")]
    Csv(#[from] csv::Error),
    #[error("yaml error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}
