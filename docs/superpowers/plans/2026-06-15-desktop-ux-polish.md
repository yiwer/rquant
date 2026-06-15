# 桌面客户端显示与交互优化 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 3 个已实现桌面页面（驾驶舱/回测中心/数据工作台 + 账本详情）的中文文案准确化、核心业务流程顺畅化——纯显示+交互打磨，业务逻辑零改动。

**Architecture:** 共享 `labels.ts`（枚举/术语单一真相源）+ `errors.ts`（后端报错→友好中文）两个可测模块作地基；逐页就地应用 + 加 must-fix 交互（dry-run 解释、run 完成自动刷新、报错友好化、路径抽象、空状态引导）。不做全量 i18n、不重布局、不动根 rquant crate 逻辑。

**Tech Stack:** React 18 + antd 6 + zustand + echarts 6 + vitest；Tauri 桥接（个别 Rust 字符串）。设计：`docs/superpowers/specs/2026-06-15-desktop-ux-polish-design.md`。UI 根目录 `desktop/ui`。

**通用纪律**：每个改组件的任务**先 Read 该组件**确认当前字符串与结构再改（精确字符串在源码里）；git add 点名；英文提交；产物在受控文件。收尾闸必须 `--workspace`（吸取桥接 crate 漏编译教训）。

---

## 文件结构

| 文件 | 责任 | 改动 |
|---|---|---|
| `desktop/ui/src/labels.ts` | 枚举/术语映射单一真相源 | 新建 + 单测 |
| `desktop/ui/src/labels.test.ts` | labels 单测 | 新建 |
| `desktop/ui/src/errors.ts` | 后端报错→友好中文 | 新建 + 单测 |
| `desktop/ui/src/errors.test.ts` | errors 单测 | 新建 |
| `desktop/ui/src/pages/Cockpit.tsx` + 子组件 | 文案 + dry-run + run 完成刷新 + 空态引导 | 改 |
| `desktop/ui/src/pages/Backtest.tsx` + 子组件 | 文案 + 模式 gloss + 报错友好 + tooltip | 改 |
| `desktop/ui/src/pages/DataBench.tsx` | 路径抽象 + 标的提取 + 因子 loading + 批量校验 | 改 |
| `desktop/ui/src/pages/BookDetail.tsx` | 13 字段中文映射 + 标题元数据下沉 | 改 |
| `desktop/src-tauri/src/*`（个别） | gate message / advice 文案 | 改 |

---

## Task 1: 共享 labels 模块 + 单测

**Files:**
- Create: `desktop/ui/src/labels.ts`、`desktop/ui/src/labels.test.ts`

- [ ] **Step 1: 写失败单测**

`desktop/ui/src/labels.test.ts`：

```ts
import { describe, it, expect } from "vitest";
import { actionZh, modeZh, snapshotFieldZh, TERM, MODE_GLOSS } from "./labels";

describe("labels", () => {
  it("maps trade actions to Chinese", () => {
    expect(actionZh("Buy")).toBe("买入");
    expect(actionZh("Sell")).toBe("卖出");
    expect(actionZh("Adjust")).toBe("调整");
    expect(actionZh("Hold")).toBe("持有");
  });
  it("falls back to raw key for unknown action", () => {
    expect(actionZh("Weird")).toBe("Weird");
  });
  it("maps run modes to Chinese", () => {
    expect(modeZh("sim_hard")).toBe("模拟·硬");
    expect(modeZh("score_soft")).toBe("打分·软");
  });
  it("maps all 13 AccountSnapshot fields", () => {
    for (const k of ["pos","entry_price","bars_held","nav","peak_nav","max_drawdown","turnover","last_increase_date","max_price_since_entry","min_price_since_entry","bars_since_exit","last_trip_return","trip"]) {
      expect(snapshotFieldZh(k)).not.toBe(k); // every field has a zh label
    }
    expect(snapshotFieldZh("entry_price")).toBe("建仓价");
  });
  it("exposes glossary terms + mode gloss", () => {
    expect(TERM.bps).toBe("基点");
    expect(TERM.warmup).toBe("热身期");
    expect(MODE_GLOSS).toContain("模拟");
  });
});
```

- [ ] **Step 2: 运行确认失败**

Run: `npm --prefix desktop/ui run test -- labels` （vitest）
Expected: 失败（`./labels` 不存在）。

- [ ] **Step 3: 实现 labels.ts**

```ts
// 桌面端显示文案的单一真相源（枚举/术语映射 + 术语表）。设计 §3。
// 量化标准术语（Sharpe/净值/留档）刻意保留，不在此强译。

export const ACTION_ZH: Record<string, string> = {
  Buy: "买入", Sell: "卖出", Adjust: "调整", Hold: "持有",
};
export const MODE_ZH: Record<string, string> = {
  sim_hard: "模拟·硬", sim_soft: "模拟·软", score_hard: "打分·硬", score_soft: "打分·软",
};
export const SNAPSHOT_FIELD_ZH: Record<string, string> = {
  pos: "仓位", entry_price: "建仓价", bars_held: "持仓根数", nav: "净值",
  peak_nav: "峰值净值", max_drawdown: "最大回撤", turnover: "换手",
  last_increase_date: "末次加仓日", max_price_since_entry: "持仓最高价",
  min_price_since_entry: "持仓最低价", bars_since_exit: "离场后根数",
  last_trip_return: "上轮回合收益", trip: "回合数",
};

export const actionZh = (k: string): string => ACTION_ZH[k] ?? k;
export const modeZh = (k: string): string => MODE_ZH[k] ?? k;
export const snapshotFieldZh = (k: string): string => SNAPSHOT_FIELD_ZH[k] ?? k;

// 一次性术语（散落标签就地引用，保持一致）
export const TERM = {
  bps: "基点", warmup: "热身期", window: "回溯窗", benchmark: "等权基准",
  bars: "根数", missing: "缺失", schtask: "计划任务", runlog: "运行日志",
} as const;

// 模式选择器一次性解释（popover）
export const MODE_GLOSS = "模拟=资金曲线 / 打分=相对排名；硬=取最优 / 软=概率加权";
```

- [ ] **Step 4: 运行确认通过**

Run: `npm --prefix desktop/ui run test -- labels`
Expected: 5 测试 PASS。

- [ ] **Step 5: 提交**

```bash
git add desktop/ui/src/labels.ts desktop/ui/src/labels.test.ts
git commit -m "feat(ux): shared labels module (action/mode/snapshot maps + glossary)"
```

---

## Task 2: 报错友好化模块 + 单测

**Files:**
- Create: `desktop/ui/src/errors.ts`、`desktop/ui/src/errors.test.ts`

- [ ] **Step 1: 写失败单测**

`desktop/ui/src/errors.test.ts`：

```ts
import { describe, it, expect } from "vitest";
import { friendlyError } from "./errors";

describe("friendlyError", () => {
  it("maps tree parse errors", () => {
    const r = friendlyError("backtest runner failed: tree parse error at line 42");
    expect(r.title).toBe("策略树解析失败");
    expect(r.detail).toContain("line 42"); // 原文保留于 detail
  });
  it("maps file-not-found", () => {
    expect(friendlyError("No such file or directory: foo.csv").title).toBe("文件未找到或无法读取");
  });
  it("maps fetch/network errors", () => {
    expect(friendlyError("tencent request error: timeout").title).toBe("数据拉取失败（网络或数据源）");
  });
  it("maps csv format errors", () => {
    expect(friendlyError("csv: row too short (3 fields)").title).toBe("数据文件格式错误");
  });
  it("falls back to generic title, keeps raw detail", () => {
    const r = friendlyError("something totally unexpected");
    expect(r.title).toBe("操作失败");
    expect(r.detail).toBe("something totally unexpected");
  });
});
```

- [ ] **Step 2: 运行确认失败**

Run: `npm --prefix desktop/ui run test -- errors`
Expected: 失败（`./errors` 不存在）。

- [ ] **Step 3: 实现 errors.ts**

```ts
// 把常见后端(Rust)报错原文映射为友好中文；原文保留于 detail 供量化用户排查。设计 §5.1。
const RULES: ReadonlyArray<readonly [RegExp, string]> = [
  [/parse|yaml|tree.*error|decision tree|expected .*found/i, "策略树解析失败"],
  [/no such file|not found|cannot find|读取失败/i, "文件未找到或无法读取"],
  [/fetch|tencent|sina|http|network|request error|timeout/i, "数据拉取失败（网络或数据源）"],
  [/csv|bad number|column|row too short|header/i, "数据文件格式错误"],
];

export function friendlyError(raw: string): { title: string; detail: string } {
  for (const [re, msg] of RULES) {
    if (re.test(raw)) return { title: msg, detail: raw };
  }
  return { title: "操作失败", detail: raw };
}
```

- [ ] **Step 4: 运行确认通过**

Run: `npm --prefix desktop/ui run test -- errors`
Expected: 5 测试 PASS。

- [ ] **Step 5: 提交**

```bash
git add desktop/ui/src/errors.ts desktop/ui/src/errors.test.ts
git commit -m "feat(ux): friendly error mapping (backend error -> zh + raw detail)"
```

---

## Task 3: 驾驶舱 Cockpit 文案 + 流程

**Files:**
- Modify: `desktop/ui/src/pages/Cockpit.tsx`、`components/BookCard.tsx`、`components/DiffTable.tsx`、`components/RunStatusPanel.tsx`（确切路径以仓库为准——**先 Read 这些文件**）

> 先 `Read` 各文件确认当前字符串。下列为"问题→修正"清单（探查自现状）。

- [ ] **Step 1: 文案修正（就地 + 引用 labels）**

逐项改：
- DiffTable：动作列值用 `actionZh(action)`（Buy→买入…）；空状态 "暂无清单(等待账本3 run)"→"暂无持仓目标（持仓组合未运行）"；列名 现权重/目标权重 保留。
- BookCard："回撤 X"→"最大回撤 X"（若已是比例显示，补 %）；"入选 N 只"→"目标持仓 N 只"；持仓/回撤/时间统计行补分隔（· 或换行），避免拥挤。
- RunStatusPanel："schtask:"→"计划任务:"；"查看 run.log"→"查看运行日志"；"未检测到 rquant-paper"→"未检测到计划任务（rquant-paper）"。

- [ ] **Step 2: dry-run 解释**

手动触发 modal：当 `gate.gate === "dry_only"`（commit 复选框被禁用）时，在禁用复选框旁加说明（Tooltip 或 Alert）：`"交易时段外或计划任务窗口冲突 → 仅可模拟运行，不写持仓状态"`。读 Cockpit.tsx 里 `runGateNow()`/`GateDto{gate,message}` 的用法接入。

- [ ] **Step 3: run 完成自动刷新 + toast**

读 `stores/cockpit.ts`（`load()`）+ TaskDrawer 的 `task://progress` 事件订阅方式。手动触发 run 启动后：订阅该任务完成事件（或任务列表中该 taskId 转 done/error），完成时调用 cockpit `load()` 重拉 + `message.success("运行完成，已刷新")`/`message.error(友好)`。消除"需手动刷新"。若事件订阅复杂，退化为：run 启动后启动一次性 20s 轮询直至该任务结束再 load。**机制以读到的 store/事件代码为准**。

- [ ] **Step 4: 空状态可执行引导**

BookCard 当账本"未建账"：在 advice 区显示可执行引导 `"账本未初始化——点上方'手动触发 run'建立首个快照"`（若 advice 文案来自桥接 DTO，则该句在 Task 7 改桥接；否则就地改）。

- [ ] **Step 5: 验证 + 提交**

Run: `npm --prefix desktop/ui run build`（tsc 通过）；目视/或 `npm --prefix desktop/ui run test`（既有组件测试不破）。
```bash
git add desktop/ui/src/pages/Cockpit.tsx desktop/ui/src/components/BookCard.tsx desktop/ui/src/components/DiffTable.tsx desktop/ui/src/components/RunStatusPanel.tsx
git commit -m "feat(ux): cockpit text zh + dry-run explainer + post-run refresh + empty guidance"
```

---

## Task 4: 回测中心 Backtest 文案 + 流程

**Files:**
- Modify: `desktop/ui/src/pages/Backtest.tsx` + `components/{BacktestConfigForm,RunHistoryList,RunOverview,TradesTable,ReplayView,CompareView}.tsx`（**先 Read**）；`api/ipc.ts`（接 errors）

- [ ] **Step 1: 文案修正（labels + 就地）**

- BacktestConfigForm：模式选择器各项 `modeZh(...)`；模式 label 旁加 Popover 内容 `MODE_GLOSS`；"warmup"→`热身期`、"window"→`回溯窗`（各加 Tooltip 说明"回测参数,非拉取参数"）；"成本bps"→`成本(基点)`；CSV 下拉信息 "${rows}根"→"共${rows}根K线"。
- RunHistoryList：模式 tag 文本用 `modeZh(...)`（保留颜色映射）。
- RunOverview："bh对照"→`等权基准`；"Sharpe" 保留，加 Tooltip "夏普比率"；长括注 "打分模式概览为原样关键字段(...)" 简化为 "打分模式：见'原始'标签查看完整字段"。
- TradesTable："持有bars"→`持仓根数`；盈亏额 tooltip "资金×trip_return,单利近似口径"→"按简单收益率近似（资金×回合收益率）"；空态 "无交易(打分模式或全程空仓)"→"无交易（打分模式或全程无持仓）"。
- ReplayView：null tag "NaN/弃权"→`缺失`。
- CompareView：标题 "(nav 口径,资金无关)" 移到副标题/Tooltip；空态 "至少一侧无曲线(打分模式)"→"至少一侧数据缺失（打分模式无资金曲线）"。

- [ ] **Step 2: tree 加载错误详情**

BacktestConfigForm 树下拉：当某树 `!t.name`（加载失败）时，除禁用选项外，在表单下方列出失败详情（树名 + 错误，若 DTO 含 error 字段则显示，否则显示"解析失败，请检查 YAML"）。读组件确认 DTO 字段。

- [ ] **Step 3: 报错友好化接入**

回测相关 `catch`/`message.error(String(e))` 处改用 `friendlyError`：
```ts
import { friendlyError } from "../errors";
// ...
const fe = friendlyError(String(e));
message.error(fe.title);
// 详情可选：Modal.error({ title: fe.title, content: fe.detail }) 或控制台 console.error(fe.detail)
```
至少回测启动失败、留档加载失败、对比加载失败三处接入。

- [ ] **Step 4: K线 info**

KlineSignalsView "末2000根" footer 旁加 info icon（Tooltip "仅展示末2000根，完整数据已参与回测"）。

- [ ] **Step 5: 验证 + 提交**

Run: `npm --prefix desktop/ui run build` + `npm --prefix desktop/ui run test`
```bash
git add desktop/ui/src/pages/Backtest.tsx desktop/ui/src/components/BacktestConfigForm.tsx desktop/ui/src/components/RunHistoryList.tsx desktop/ui/src/components/RunOverview.tsx desktop/ui/src/components/TradesTable.tsx desktop/ui/src/components/ReplayView.tsx desktop/ui/src/components/CompareView.tsx desktop/ui/src/api/ipc.ts
git commit -m "feat(ux): backtest text zh + mode gloss + friendly errors + tree-error detail + tooltips"
```

---

## Task 5: 数据工作台 DataBench 文案 + 流程

**Files:**
- Modify: `desktop/ui/src/pages/DataBench.tsx`（**先 Read**）

- [ ] **Step 1: 路径抽象 + 标的提取**

- 卡标题 "行情 CSV(paper/ + .rquant-desktop/data/)"→"行情数据库"；"批量拉取(新浪 qfq → .rquant-desktop/data/)"→"批量拉取（新浪 qfq）"。
- CSV 列表：从路径提取标的+周期当主标题（如 `sh600030 · 60m`），原路径作次要灰字/Tooltip。提取规则：basename 去 `p_`/`pd_` 前缀与 `.csv` 后缀取 symbol；scale 若文件名/DTO 有则显示。读 DTO 确认可用字段（可能已有 symbol/scale 字段，优先用）。
- universe "deploy 只读" tag→"内置"。
- "串行+500ms 节流;进度见任务抽屉"→"逐个拉取（节流）；进度见任务抽屉"。
- 因子叠加描述 "...同口径求值(NaN 断线=弃权)"→"...同口径求值（NaN 无法计算，显示断线）"。

- [ ] **Step 2: 因子叠加 loading 反馈**

"叠加因子" 按钮：求值期间 `loading` 态（按钮 spinner + 禁用），完成/失败恢复。读 DataBench 里因子求值（`data_eval_factor`）调用接入 loading state。失败用 `friendlyError`。

- [ ] **Step 3: 批量拉取输入校验 + 预览**

输入框下方实时预览解析出的标的列表（`fetchSyms.split(/[,\s]+/).filter(Boolean)`）："将拉取: sh600030, sz000001"；非法格式（不匹配 `^(sh|sz|bj)\d{6}$`）标红提示。拉取按钮在无有效标的时禁用。

- [ ] **Step 4: 验证 + 提交**

Run: `npm --prefix desktop/ui run build` + `npm --prefix desktop/ui run test`
```bash
git add desktop/ui/src/pages/DataBench.tsx
git commit -m "feat(ux): databench path abstraction + symbol extraction + factor loading + fetch validation"
```

---

## Task 6: 账本详情 BookDetail 文案

**Files:**
- Modify: `desktop/ui/src/pages/BookDetail.tsx`（**先 Read**）

- [ ] **Step 1: 13 字段中文映射 + 标题元数据下沉**

- AccountSnapshot 字段渲染用 `snapshotFieldZh(key)` 作中文表头（保留原值；英文 key 可作次要灰字/Tooltip 供对照）。
- 标题元数据下沉：`"AccountSnapshot(13 字段,只读)"`→ 标题 "持仓快照" + 副标题/小字 "只读 · 13 字段"；journal 卡标题的括注（"自桌面端启用日积累"）下沉为副标题或卡内说明。

- [ ] **Step 2: 验证 + 提交**

Run: `npm --prefix desktop/ui run build` + `npm --prefix desktop/ui run test`
```bash
git add desktop/ui/src/pages/BookDetail.tsx
git commit -m "feat(ux): book detail snapshot field zh map + title metadata to subtitle"
```

---

## Task 7: 桥接源字符串中文化（如有）

**Files:**
- Modify: `desktop/src-tauri/src/*`（gate message / 账本 advice 文案——**先 grep 定位**）

- [ ] **Step 1: 定位面向用户的中文/英文串**

`grep` 桥接 crate 里返回给前端的用户可见文案：gate message（`run_gate_now` 的 `GateDto.message`）、账本 advice（cockpit overview 的 advice 字段）。确认哪些直接显示给用户。

- [ ] **Step 2: 文案修正**

把这些串改为与前端一致的准确中文（如 gate dry_only message → "交易时段外或计划任务窗口冲突，仅可模拟运行"；advice 未建账 → "账本未初始化——运行手动触发以建立首个快照"）。若这些文案已在前端覆盖（Task 3/4 已就地改），则本任务仅确认无重复/矛盾、可跳过实际改动并说明。

- [ ] **Step 3: 验证 + 提交**

Run: `cargo build -p rquant-desktop`（桥接编译过）
```bash
git add desktop/src-tauri/src/<改动文件>
git commit -m "feat(ux): bridge user-facing strings zh accuracy"
```

> 若无桥接字符串需改（全在前端覆盖），跳过提交并在报告说明。

---

## Task 8: 全量收尾闸（--workspace）

**Files:** 无（验证）

- [ ] **Step 1: 前端构建 + 测试**

Run: `npm --prefix desktop/ui run build`（tsc 零错误）
Run: `npm --prefix desktop/ui run test`（labels/errors 新测 + 既有组件测试全绿）

- [ ] **Step 2: workspace 闸（吸取桥接 crate 漏编译教训）**

Run: `cargo test --workspace`
Expected: 全绿（根 333 + 桥接 89 + e2e 等，0 失败——若 Task 7 改了桥接、确认其测试仍过）。
Run: `cargo clippy --workspace --all-targets`
Expected: 零警告。

- [ ] **Step 3: GUI 运行时冒烟（可选但推荐）**

按既有两进程启动（`npm --prefix desktop/ui run dev` + `cargo run -p rquant-desktop`），目视确认：驾驶舱中文文案到位（动作中文、最大回撤、计划任务）、回测中心模式 gloss/tooltip、数据工作台标的提取/路径抽象、账本详情 13 字段中文。截图存证。完后清理进程（kill rquant-desktop + vite，释放 5173）。

- [ ] **Step 4: 最终确认**

`git status --porcelain` 干净；本弧线仅改 desktop/ui + 个别 desktop/src-tauri + 文档，无根 src/ 业务逻辑改动。

---

## Self-Review（写计划后自查）

**Spec 覆盖**：labels 模块（§3）→ Task 1；errors（§5.1）→ Task 2；驾驶舱文案+流程（§4.1）→ Task 3；回测（§4.2）→ Task 4；数据工作台（§4.3）→ Task 5；账本详情（§4.4）→ Task 6；桥接串（§4.5）→ Task 7；测试+--workspace 闸（§5.2）→ Task 8。边界（§5.3）贯穿（不动逻辑/不重布局/不碰 5 占位）。✅ 全覆盖。

**占位符扫描**：labels.ts/errors.ts 给了完整代码 + 完整单测；逐页任务给"问题→修正"精确清单（探查自现状），并明确"先 Read 组件"——这不是占位，是 UI 打磨的恰当粒度（精确字符串在源码、实现者读后就地应用，散串无法也不必在计划里逐行重抄）。流程改动（run 完成刷新、tree 错误详情）给了机制 + 文件 + 事件名 + 退化方案，"以读到的 store/事件代码为准"是对未读源码的诚实指引非含糊。

**一致性**：`actionZh/modeZh/snapshotFieldZh/TERM/MODE_GLOSS`（Task 1）与各页引用一致；`friendlyError(raw)->{title,detail}`（Task 2）与 Task 3/4/5 接入一致；改动文件表与各 Task 的 git add 一致；--workspace 闸与设计 §5.2 一致。✅
