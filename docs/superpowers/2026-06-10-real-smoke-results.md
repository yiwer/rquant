# 真实端点 smoke 结果（2026-06-10）

全管线对真实外部服务的手动验证：新浪行情 + DeepSeek + DashScope。master @ `04ff9a7`。

## P1 取数（新浪）

- **发现并修复**：M6 时的默认端点 `money.finance.sina.com.cn/quotes_service/api/json_v2.php` 已回 `{"__ERROR":3,"__ERRORMSG":"Service not valid"}`；`quotes.sina.cn/cn/api/json_v2.php` 可用。默认值已修（`04ff9a7`），`--base-url` 覆盖机制在排障中即发挥了设计作用。
- sh600000 15m/60m 与 sz000001 60m 各拉满 1023 根（2026-03-06 → 2026-06-10 盘中）；真实 payload 多出的 `amount` 字段被 serde 正常忽略；string→f64、升序、CSV 往返全部正确。
- 重试+净错误链按设计工作（端点死时重试 2 次后干净报错、无 panic）。

## P2 回测/报告（真实数据，无 LLM）

- **缺口检测实战命中**：4 个缺失交易日（2026 清明/五一区间），无 `--holidays` 提示正确出现。
- 硬（trend_tree）：923 决策 / 907 计分；示例树条件未触发（active=0，符合该树为合成演示设计）。
- 软（strength_tree）：engaged n=357；**position ≡ engaged 在真实数据上成立**（long/flat 等价不变量）；策略本身无 edge（mean −0.0007、hit 39.5%、buy&hold −7.1%）——诚实结果，smoke 验管线不验策略。
- 硬/软 HTML 报告均正确渲染（软 56KB：907 点曲线 + 直方图 + 堆叠面积图 + position headline）。

## P3 真实 LLM（probs 协议验证，每家 23 个决策点）

| Provider | 模型 | 结果 |
|---|---|---|
| DeepSeek | deepseek-v4-pro | ✅ 完整遵守 probs 协议：`{"probs":{"down":0.1,"sideways":0.25,"up":0.65},"reason":...}` |
| DashScope | qwen3.7-max | ✅ 同样遵守：`{"probs":{"down":0.1,"sideways":0.8,"up":0.1},"reason":...}` |

- 3-label 树（up/down/sideways）真正行使多路：硬 traces 显示 argmax + 真实置信（如 0.45）与推理文本；软 traces 显示真实多路 leaf_probs（up→leaf_long 0.45/0.7，down+sideways 合并→leaf_flat）。
- **缓存**：23 个内容寻址条目/每家，存清洗后分布 `{probs, reason, model}`；同数据二跑（软模式）全命中、零新调用、秒回。
- 软 vs 硬在同一真实 LLM 判断上的差异符合预期：硬 active n=3（仅 argmax=up 的点入场），软 engaged n=7（所有带 long 质量的点按比例参与）。
- 解析清洗未触发回退（两家输出均合法）；JSON mode + 明确 prompt 足够稳。

## 结论

fetch → backtest（硬/软/缺口/LLM probs）→ traces → report 全链路在真实世界可用。唯一发现的缺陷（默认新浪端点失效）已当场修复合并。无遗留问题。

> 运行方式备忘：smoke 产物在 `tmpsmoke/`（已清理，不入库）；LLM key 经用户机器级 env（`DEEPSEEK_API_KEY`/`DASHSCOPE_API_KEY`）桥接至 `RQUANT_LLM_API_KEY`，未落对话/仓库。
