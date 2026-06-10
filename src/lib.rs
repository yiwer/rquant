//! **rquant** — 模糊决策树 A股回测引擎。
//!
//! 用户以 YAML 编写决策树（量化 DSL 条件节点 + LLM 节点），引擎逐 bar 遍历树、
//! 计算前瞻收益并打分；支持硬遍历（deterministic argmax）与软遍历（叶子概率分布）。
//! 复现性第一：所有随机性均通过 stub/cache 封装，测试不依赖外部服务。
//!
//! 参考文档：`docs/architecture.md`（模块总览）、`docs/tree-yaml-schema.md`（YAML 格式）、
//! `docs/dsl-reference.md`（DSL 语法）、`docs/llm-protocol.md`（LLM 节点协议）。

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
    /// 行情数据读取或解析失败。
    #[error("data error: {0}")]
    Data(String),
    /// DSL 词法/语法/求值错误。
    #[error("dsl error: {0}")]
    Dsl(String),
    /// 决策树加载或验证错误（DAG、可达性、stance 等）。
    #[error("tree error: {0}")]
    Tree(String),
    /// 节点求值错误（量化或 LLM）。
    #[error("eval error: {0}")]
    Eval(String),
    /// 引擎遍历错误。
    #[error("engine error: {0}")]
    Engine(String),
    /// 回测运行错误。
    #[error("backtest error: {0}")]
    Backtest(String),
    /// 底层 I/O 错误。
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// CSV 解析错误。
    #[error("csv error: {0}")]
    Csv(#[from] csv::Error),
    /// YAML 反序列化错误。
    #[error("yaml error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    /// JSON 序列化/反序列化错误。
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}
