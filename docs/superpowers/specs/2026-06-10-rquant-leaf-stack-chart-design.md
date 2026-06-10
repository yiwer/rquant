# rquant：叶子概率堆叠面积图 — 设计文档

- **日期**：2026-06-10
- **状态**：设计已确认（三特性批次之②；①净仓位已合并 `8d71238`）
- **关联**：`report --soft` 已渲染曲线/直方图/avg_leaf 条形；soft traces 每点含 `leaf_probs`（Σ=1）。本设计补"质量如何随时间在叶子间转移"的视图。

---

## 1. 目标与非目标

### 目标
1. `report/curve.rs` 加 `leaf_prob_stack(records) -> StackSeries`：`names`（全体叶名并集，字典序）+ `rows`（每点各层**累计边界**，末层≈1）。
2. `report/viz.rs` 加 `stacked_area_chart(&StackSeries, title)`：每层一个 `<polygon>`（上边界=本层累计、下边界=前层累计），y 域固定 [0,1]，固定 6 色调色板循环 + 图例；确定性。
3. `render_soft_html` 加参 `stack: Option<&StackSeries>`（无 traces → None 跳过）；cli `report --soft` 构建并传入；涟漪（viz 测试、e2e）一并改。

### 非目标（YAGNI）
- 交互/悬浮提示；自适应配色；硬模式版本；降采样（先全画）。

## 2. 锁定决策

| # | 维度 | 选定 |
|---|---|---|
| 1 | 落点 | 进 `report --soft`（曲线/直方图之后），不另出文件 |
| 2 | 分层 | 叶名字典序；固定 6 色调色板循环；右上图例 |
| 3 | 数据 | `rows[i][k] = Σ_{j≤k} leaf_probs[names[j]]`（缺省 0）；y 域 [0,1] 恒满幅 |

## 3. 实现要点
- `StackSeries { pub names: Vec<String>, pub rows: Vec<Vec<f64>> }`。
- 多边形：layer k 的点 = 上边界 `(x_i, rows[i][k])` 正序 + 下边界 `(x_i, rows[i][k-1]，k=0 时 0)` 逆序；x 按索引均匀铺开（n=1 时退化竖条，分母 `max(n-1,1)`）。
- 调色板 `const PALETTE: [&str; 6]`（如 `#1565c0 #2e7d32 #c62828 #f9a825 #6a1b9a #00838f`）。
- 空 records → "no data" 占位（同既有图元约定）。
- 涟漪：`render_soft_html` 签名变 → viz 自包含测试、cli soft 分支、e2e `soft_report_html_renders` 三处调用点补 `stack` 实参。

## 4. 测试
- `leaf_prob_stack`：两点两叶已知 records → names 排序、rows 累计边界正确、末层≈1；空 → 空。
- `stacked_area_chart`：含 `<polygon>` 与图例文本；同输入同字节。
- `render_soft_html(Some(stack))` 含 `<polygon>`；`None` 不含且不 panic。
- e2e：软全链路 HTML 含 `<polygon>`。

## 5. 里程碑
- **T1** `curve.rs` `StackSeries`/`leaf_prob_stack` + 测试。
- **T2** `viz.rs` `stacked_area_chart` + `render_soft_html` 加参 + cli/e2e/viz 测试涟漪 + README 一句。
