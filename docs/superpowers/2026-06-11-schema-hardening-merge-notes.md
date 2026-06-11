# worktree-schema-hardening → master 合并指南（2026-06-11）

分支 `worktree-schema-hardening`（基底 de51958，27 提交，G 系列 Schema 表达力补强全部完成：244 lib + 18 e2e 全绿、clippy -D warnings 零告警）。master 并行落地了 F-2（optimize/WFO）与 F-9（signal），合并时按下表处理。**建议 `git merge`（一次性解决），不要 rebase**（27 提交逐个重解同一批冲突）。

## 文本冲突（3 处）

| 文件 | 性质 | 解法 |
|---|---|---|
| `src/backtest/sim.rs` | 结构体字段 / sim_step 签名 / 测试块 + **语义** | 两边都保留：本分支的 high/low 参数与极值字段 + master 的 snapshot/restore；**必须扩展 `AccountSnapshot` 纳入 `max_price_since_entry`/`min_price_since_entry` 并更新其 roundtrip 测试**——否则纸面交易快照恢复后极值重置 NaN，Chandelier/跟踪止损在恢复后首根 bar 静默失效 |
| `src/tree/loader.rs` | 同区域编辑 | 两边保留：master 的 `load_tree_str_with_overrides` 包装 + 本分支的 Weight/judges/Cached/保留名——不同函数，无逻辑重叠 |
| `docs/cli-reference.md` | 琐碎 | 两边保留：master 的 signal/optimize 节 + 本分支的 --aux 时间戳段 |

## 静默编译破坏（无文本冲突，merge 后必现编译错——这是预期信号，不是事故）

| 文件 | 破坏点 | 机械修复 |
|---|---|---|
| `src/signal/mod.rs` | 1 处 `sim_step(...)` 旧 8 参；2 处 `leaf.weight` 字段访问 | sim_step 补执行 bar 的 `high, low` 实参（插在 open 与 close 之间）；`.weight` → `.weight_at(&ctx)` |
| `src/optimize/mod.rs` | 1 处 `sim_step(...)`；2 处 `leaf.weight`（约 :95、:253） | 同上 |

另：master 测试若有手写 `Context { ... }` 字面量，补 `eval_cache: Default::default(),`。

## 合并后验收门

```
cargo test && cargo clippy --all-targets -- -D warnings
```

静默破坏会以编译错形式立刻浮出——绿灯即合并正确性门。建议再跑一次 AccountSnapshot roundtrip 测试确认极值字段持久化。

## 本分支交付清单（报告项 → 实现）

1. `count(cond,n)` / `barssince(cond)`（逐位布尔序列、尾对齐、NaN 逐位弃权、crossover/crossunder 事件序列）+ `abs`/`min`/`max`（显式 NaN 传播）
2. `max/min_price_since_entry` 持仓极值状态量（Chandelier 可表达；含入场执行 bar；空仓/翻向 NaN）
3. 叶子 `weight` DSL 表达式（金字塔加减仓：`"min(1, pos + unit)"`；NaN→0 弃权、clamp [0,1]、带引号纯数字坍缩为校验常量）
4. 顶层 `judges:` 块（LLM 判定复用：物化 label→goto、缓存 scope=`judge:<名>`、每 bar 每 judge 一次网络调用）
5. 因子按决策点 memoize（`Expr::Cached` 槽位 + `Context.eval_cache`；语义严格等价有测试锁定）
6. aux 时间戳纪律文档（as-of join、行时间=数值可知晓时刻、正误打戳对照表）+ 数据层拒绝非有限 OHLCV

注：报告第 1 项的 `ref(expr,k)` 在计划核对时确认**已存在**（eval.rs，含 Turtle 突破测试），缺口收窄为 count/barssince。
