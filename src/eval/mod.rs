//! 节点求值：量化分支求值（硬/软）及 LLM 节点调用，统一输出 [`Decision`]。

pub mod quant;
pub mod llm;

/// 单次节点求值的结果，携带路由目标、标签、置信度和推理说明。
#[derive(Debug, Clone)]
pub struct Decision {
    /// 下一跳节点或叶子的 ID。
    pub goto: String,
    /// 触发的分支标签（量化命中标签 / LLM 胜出标签 / `"default"`）。
    pub label: String,
    /// 置信度语义因节点类型而异：
    /// - 量化节点：分支命中时为 `1.0`，走 default 时为 `0.5`。
    /// - LLM 节点：胜出 label 的概率；若残余概率使 `"default"` 胜出，
    ///   则为该残余值，`label` 同时为 `"default"`。
    pub confidence: f64,
    /// 人类可读的求值说明，便于 trace 调试。
    pub rationale: String,
}
