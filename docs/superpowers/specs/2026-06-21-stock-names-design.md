# 股票中文名全局展示 设计(stock-names)

> 状态:已 brainstorm 定稿,待 writing-plans。日期:2026-06-21。
> 前序:客户端 sub-1/2a/3a + task-ux + process-audit 已合入 master。复用桌面范式。

## 0. 背景

用户:"股票除了代码可以展示其中文名称吗?",范围=**所有出现代码处**。现状:磁盘无 symbol→中文名 文件(`universe_baostock_day.csv` 仅 symbol+路径;`sector_membership.csv` 仅 symbol+行业,抓取时读到的 `code_name` 未保留)。名称可得:baostock `query_stock_basic`(全市场一次返回 code/code_name)或 `build_sectors` 已读的 code_name。

## 1. 决策(brainstorm 定论)

| 决策 | 结论 |
|---|---|
| 范围 | **所有出现 symbol 的渲染点**加中文名。 |
| 机制 | **方案 B:共享名称映射 + 共享 `<SymbolLabel>` 组件**,不逐 DTO 加 `name` 字段(桥层零 DTO 改动、DRY、加点只换渲染)。 |
| 数据 | `scripts/build_names.py`(baostock 一次性/周期抓)→ `data/baostock/names.csv`(`symbol,name`);脚本提交、CSV gitignored;实现时联网抓一次。 |

## 2. 架构

### 2.1 数据(`scripts/build_names.py`)
- baostock `login` → `query_stock_basic()`(无参=全市场)→ 行 `code(sh.600000), code_name, ipoDate, outDate, type, status`。筛 `type=='1'`(股票)& 可选 `status=='1'`(上市);`code` 去点归一为 `sh600000`(与 `sector_membership.csv` 同口径);写 `data/baostock/names.csv` 表头 `symbol,name`,UTF-8。一次调用即可(无需逐股)。退化:baostock 不可用则脚本报错退出(不写半文件)。
- `.gitignore` 已忽略 `data/`;脚本提交。

### 2.2 桥层
- `paths::Workspace::names_path()` = `data/baostock/names.csv`。
- `names.rs`:`load_names(path) -> std::collections::HashMap<String,String>`(读 CSV,跳表头/坏行,缺文件→空 map,容错)+ 纯逻辑可 TDD。
- 命令 `names_map(state) -> std::collections::HashMap<String,String>`(serde 序列化为 JS 对象;**无需新 ts-rs DTO**)。文件小(~5000 行),每次调用现读即可(或进程内缓存;现读够用)。在 `generate_handler!` 注册。

### 2.3 前端
- `api/ipc.ts`:`namesMap: () => invoke<Record<string,string>>("names_map")`。
- `stores/names.ts`(zustand):`{ names: Record<string,string>, loaded: bool, load(): Promise<void> }`;`App` 启动调一次 `load()`(同 tasks store init)。导出访问器 `symbolName(sym: string): string | undefined`(从 store 读,未加载/缺名→undefined)。
- `components/SymbolLabel.tsx`:`props { symbol: string; nameFirst?: boolean }`;渲染 `名称`(主)+ `代码`(次/灰),名缺则只显代码。例:`民生银行 sh600016`(灰 code)。纯展示,读 names store。
- **替换所有 symbol 渲染点**为 `<SymbolLabel symbol={sym}/>`:`ScreenPickTable`(选股榜)、`DiffTable`(部署/驾驶舱组合 diff)、`Deploy.tsx` 持仓列表、`pages/Cockpit` 组合 diff(若直接渲染 symbol)、回测 `TradeDto` 交易表(symbol 列)、`ReplayView`(symbol)、`ScreenBacktestResult` 若列 symbol、`BookDetail` 持仓(若有)。逐一 READ 确认渲染处。表格类可加独立「名称」列或在 symbol 单元格并排;文本/行内用 `代码 名称`。

## 3. 数据流

- 启动:`App` → `names.load()` → `api.namesMap()` → store。
- 渲染:任意 `<SymbolLabel symbol/>` → `symbolName(sym)` 查 store → `名称 代码` 或仅代码。
- 刷新:重跑 `build_names.py` 更新 names.csv;app 重启(或 names.load 再调)生效。非实时。

## 4. 错误处理(诚实)

- names.csv 缺/某 symbol 无名 → 只显代码,**不臆造**。
- names store 加载失败(命令报错)→ 全局只显代码,app 不受影响(`load` catch 静默,沿用 tasks store init 容错)。
- build_names 抓取失败 → 报错退出不写半文件;names.csv 不存在时桥层返空 map。

## 5. 测试

- Rust:`names.rs` `load_names` 单测(正常 CSV→map、缺文件→空、坏行跳过、表头跳过)。
- 前端:`stores/names.ts`(注入 mock api,load 填 names;symbolName 命中/未命中)+ `SymbolLabel.tsx`(有名→名称+代码、无名→仅代码)vitest。
- 收尾:`cargo test --workspace` + tsc/vitest/build 全绿;真数据:跑 `build_names.py` 生成 names.csv(核对 sh600016→民生银行 等);GUI 冒烟(release):选股榜/部署持仓显示中文名,缺名股只显代码。

## 6. 范围边界(YAGNI)

不含:实时名称(周期抓即可)、按名称模糊搜索/筛选(后续可选)、每个 DTO 加 `name` 字段(用共享映射替代);names 仅 A 股(baostock 范围)。
