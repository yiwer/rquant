# 全仓审查发现与加固计划（2026-06-10）

三路并行审查（代码/架构、文档准确性、测试覆盖）@ master `58ae1b8`。本文是发现汇总与行动记录；执行在 `review-hardening` 分支。

## 真行为 bug
- **B1** DSL `!=` 预热误触发：`as_scalar` 预热归约 NaN，注释称"比较为 false→弃权"，但 `NaN != x` 为 true → `when: "close != 5"` 预热期触发。修：Eq/Ne 显式 NaN→false + 修注释（实际只有 Ne 行为变化；Eq 的 NaN==x 本就 false）。

## Reviewer 误判（经核验不修）
- "缺口告警硬/软漂移"：有意差异（SoftReport 无 gaps 字段，省略 "(see report.gaps)" 是 spec 设计）。
- "buy_and_hold len==1 风险"：`len>=2` guard 同时罩住两个访问，现状安全（仅做对称化 nit）。

## 代码卫生（P2）
W2 overlap_warning 常量化；W3 CLI `Report` 臂 56 行业务逻辑下沉 `report::render_report_files`；W4-W7 内部类型 pub→pub(crate)（Parser/schema specs/LLM prompt+cache）；W9 `push_str(&format!)`→`write!`；C2 calendar 会话时间 LazyLock 化；C1 对称化；N1 copied、N2 &Token、N3 const 提升。

## 测试缺口（P3）
HIGH：H1 `>=`/`<=`（含 fuzzy）；H2 root 非节点；H3 不可达节点；H4 LLM 启用守卫（抽纯函数）；H5 Report --soft 渲染路径（经 W3 抽出的函数测）；H6 硬模式 folds。MEDIUM 收：M1 eval 层 dispatch（rsi/atr/ema/crossover/crossunder）、M3/M4 坏时间字符串、M5 holidays 集成、M6 并发 cache put、M7 report JSON 字节等价。

## 文档（P4）
新文档：`docs/dsl-reference.md`（16 函数+运算符+归约语义）、`docs/tree-yaml-schema.md`、`docs/cli-reference.md`、`docs/architecture.md`（当前态+spec 偏离清单）、`docs/llm-protocol.md`。README 重构（10 勘误 + Quick Start/flags 表结构）。rustdoc：crate 级 `//!`、mod 级 `//!`、核心 pub 类型 `///`。注释修正：as_scalar NaN 语义（随 B1）、soft.rs "必 Some" 说明、engine/soft 两阶段措辞。

## 执行
P1 行为修复（TDD）→ P2 代码卫生 → P3 测试补全 → P4 文档。逐包子代理，全绿合并。
