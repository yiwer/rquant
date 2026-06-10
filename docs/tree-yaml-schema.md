# 决策树 YAML Schema 参考

本文档基于 `src/tree/{schema,loader}.rs` 的实际代码整理。

---

## 顶层结构

```yaml
meta:          # 树元数据（必填）
  name: "..."
  forward_window: 16
  stances: [long, flat]

root: "node_id"    # 根节点 id（必须是 nodes 中的键，不能是叶子）

nodes:             # 节点映射（HashMap，YAML 键顺序不影响语义）
  node_id:
    type: quant | llm
    # ... 节点字段

leaves:            # 叶子映射
  leaf_id:
    stance: long | flat | short
```

---

## `meta` 块

| 字段 | 类型 | 说明 |
|---|---|---|
| `name` | string | 树的可读名称，写入报告 |
| `forward_window` | usize | 前瞻窗口（primary bar 根数），用于前瞻收益评分 |
| `stances` | `[long\|flat\|short, ...]` | 允许的立场词表；叶子 stance 必须在此列表内 |

---

## `nodes` — quant 节点

```yaml
node_id:
  type: quant
  branches:
    - when: "<DSL 布尔表达式>"
      strength: "<可选>"   # 见下方 strength 说明
      goto: "<目标 id>"
      label: "<分支名>"
    - when: "..."
      goto: "..."
      label: "..."
  default:
    goto: "<目标 id>"
    label: "<分支名>"
```

**求值语义**：分支按声明顺序依次求值，**第一个 `when` 为真的分支胜出**（硬遍历）。没有任何分支命中时走 `default`。

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `branches` | 列表 | 是 | 有序分支列表 |
| `branches[i].when` | string | 是 | DSL 布尔表达式，在加载时编译 |
| `branches[i].strength` | string | 否 | 强度表达式（软模式用），见下方 |
| `branches[i].goto` | string | 是 | 目标节点或叶子 id |
| `branches[i].label` | string | 是 | 分支标签，写入 Trace |
| `default.goto` | string | 是 | 无分支命中时的目标 |
| `default.label` | string | 是 | 走 default 时写入 Trace 的标签 |

---

## `nodes` — llm 节点

```yaml
node_id:
  type: llm
  inputs: [news_score, recent_headlines]   # 可选，注入 user message 的 Context 字段
  prompt: "判断指令文本"
  labels:
    label_name: "<目标 id>"
    other_label: "<目标 id>"
  default: "<目标 id>"   # LLM 不可用/出错/弃权时的兜底目标
```

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `inputs` | `[string, ...]` | 否，默认 `[]` | 追加到 user message 的 Context 字段名；目前支持 `news_score`、`recent_headlines` |
| `prompt` | string | 否，默认 `""` | 注入 user message 的判断指令 |
| `labels` | `{label: target_id}` | 否，默认 `{}` | LLM 输出 label 到目标 id 的映射 |
| `default` | string | 是 | 兜底目标（LLM disabled/错误/弃权时使用） |

---

## `leaves`

```yaml
leaves:
  leaf_id:
    stance: long | flat | short
```

叶子的 `stance` 必须在 `meta.stances` 中声明。

---

## `strength` 字段详解

`strength` 是 quant 分支的可选软模式字段，在 **`--soft` 遍历**时生效（硬遍历忽略）。

### 三种形式

| 形式 | 解析为 | 说明 |
|---|---|---|
| `"auto"` | `Auto(0.02)` | 对该支的 `when` 表达式做模糊求值，默认 scale=0.02 |
| `"auto(0.05)"` | `Auto(0.05)` | 指定 scale 的模糊求值，scale 必须 > 0 |
| `"sigmoid((close - sma(close,20)) * 50)"` | `Expr(...)` | 任意 DSL 标量表达式，加载时编译 |

### 软模式下的首真泄漏语义

软遍历中，quant 节点按有序分支做**首真泄漏**（`quant_branch_dist`，`eval/quant.rs`）：

```
remaining = 1.0
对每个 when==true 的分支 i（按顺序）：
  raw_strength = 求值 strength（Expr/Auto；NaN→0；clamp[0,1]）
  w_i = remaining × raw_strength
  remaining = remaining × (1 - raw_strength)
  若 remaining ≤ 1e-12 则停止
最终将 remaining 分配给 default
```

- 无 `strength` 时等价于 `strength=1.0`（软模式退化为硬首真，remaining 全给当前分支）。
- NaN strength 归 0（等价于跳过该分支，remaining 全给后续）。
- 所有权重之和恒为 1.0。

### 示例

```yaml
branches:
  - when: "close > sma(close,20)"
    strength: "sigmoid((close - sma(close,20)) / (0.02 * sma(close,20)))"
    goto: leaf_long
    label: above_ma
```

参见 `examples/strength_tree.yaml`。

---

## 校验规则（`validate`，`loader.rs`）

加载时执行以下全部检查，任何一项不通过则报错：

1. **root 必须是节点**：`root` 键必须在 `nodes` 中，不能是 `leaves` 中的叶子。
2. **无悬空引用**：所有 `goto` / `labels` 目标必须存在于 `nodes` 或 `leaves` 中。
3. **所有节点从 root 可达**：BFS 从 root 出发，发现不可达节点则报错（防孤岛节点积累）。
4. **无环（DAG 检查）**：DFS 染色，检测到后向边（颜色=in-stack）即报错。
5. **叶子 stance 合法**：每个叶子的 `stance` 必须在 `meta.stances` 中。
6. **strength 表达式可编译**：`strength` 字段若存在，在加载时解析为 DSL Expr 或 Auto(scale)；格式错误立即报错。

---

## 最小合法示例

```yaml
meta:
  name: "minimal"
  forward_window: 16
  stances: [long, flat]

root: check

nodes:
  check:
    type: quant
    branches:
      - when: "close > sma(close,20)"
        goto: leaf_long
        label: up
    default:
      goto: leaf_flat
      label: none

leaves:
  leaf_long: { stance: long }
  leaf_flat: { stance: flat }
```

---

## 参考示例

- `examples/trend_tree.yaml`：两级量化节点 + 一个 LLM 节点的完整树
- `examples/strength_tree.yaml`：带显式 strength 表达式的软模式示例
