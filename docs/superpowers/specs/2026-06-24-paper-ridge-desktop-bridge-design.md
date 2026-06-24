# 桌面「纸面盘」页 — 去相关岭组合 + 岭值双引擎对照 设计

> 日期 2026-06-24 · 状态 已获用户批准(设计 OK,全功能,历史回测对照)

## 目标

把已验证的 ridge-on-gauss 选股策略(「去相关岭组合」)接进桌面 app,新增「纸面盘」页:
展示本周 top-3 选股 + 前向纸面册(开/平仓历史 + 累计 NAV/超额 vs csi300)+ 岭值双引擎 6 折回测对照,
并提供 **推进纸面册 / 重训权重 / 重算对照** 三个写操作。**复用已验证 Python 逻辑,不重写;最小侵入桌面后端。**

## 架构(方案 A)

Rust 命令**直接读 Python 产物**算状态 DTO(只读、秒开、零子进程);写操作走现有 `TaskRegistry` shell Python。
完全新文件承载逻辑,与 gm_tail WIP 不重叠。

## 组件

### Python(改动极小)
- `eval_blend.py`:加 `--json <path>` —— 把现有 6 折表 + 均值(corr/Sharpe/maxDD/excess,ridge/value/blend)
  dump 成 JSON 供桌面缓存读取。打印逻辑不变,仅多一个产物。
- `paper_ridge.py`:**不改**。Rust 读它已写的 `data/factor_panel/paper_ridge_journal.csv` +
  `paper_ridge_weights.json`;推进/重训直接 shell 它(`python scripts/paper_ridge.py [--retrain]`)原样。

### Rust(全新文件,不碰 commands.rs/dto.rs)
- `desktop/src-tauri/src/dto_paper.rs`(`#[derive(Serialize, TS)]`):
  - `PaperPickDto { symbol }`(或直接 `Vec<String>`)
  - `PaperRowDto { date, status, picks: Vec<String>, turnover, gross_ret: Option<f64>, net_ret: Option<f64>, nav: f64 }`
  - `BlendFoldDto { oos, corr, sh_ridge, sh_val, sh_blend, dd_ridge, dd_val, dd_blend, ex_ridge, ex_val, ex_blend }`
  - `BlendDto { folds: Vec<BlendFoldDto>, mean_corr, sh_ridge, sh_val, sh_blend, dd_ridge, dd_val, dd_blend, ex_ridge, ex_val, ex_blend }`
  - `PaperStatusDto { initialized: bool, strategy, train_lo, train_hi, n_train_dates, delta, top_n, cost_bps,
    open_picks: Vec<String>, closed: Vec<PaperRowDto>, cum_net: f64, cum_excess: Option<f64>, blend: Option<BlendDto> }`
- `desktop/src-tauri/src/paper_cmds.rs`:
  - `paper_ridge_status(state) -> Result<PaperStatusDto, String>`:读 weights.json(元信息)+ journal.csv
    (逐行→ closed 算 nav=cumprod(1+net),open 进 open_picks;cum_excess 复用 `index_relative` 读 csi300)
    + blend.json(若存在)。无 weights → `initialized:false`。**纯读、light、无 task。**
  - `paper_ridge_advance(state) -> Result<String,String>`:`tasks.start("paper_advance", true, |ctx| shell
    python scripts/paper_ridge.py)`,流式 stdout 进度,完成返回 task id。
  - `paper_ridge_retrain(state) -> Result<String,String>`:同上 + `--retrain`。
  - `paper_blend_recompute(state) -> Result<String,String>`:shell `python scripts/eval_blend.py --json
    data/factor_panel/paper_blend.json`(heavy,~数分钟)。
  - 复用 `paths::python_exe()` / `ws.root()` / `TaskCtx` 模式(同 `iter_cmds.rs`)。
- `lib.rs`:**新增**(与 gm_tail 行空间分离)`pub mod paper_cmds; pub mod dto_paper;` + handler 四条
  `paper_cmds::paper_ridge_status/_advance/_retrain, paper_cmds::paper_blend_recompute`。

### UI(新页 + 干净文件)
- `desktop/ui/src/pages/PaperRidge.tsx`:分区 ① 冻结权重元信息 ② 本周持仓 top-3 ③ 纸面册表 +
  累计净收益/超额 ④ 岭值双引擎 6 折对照表 ⑤ 操作按钮(推进/重训/重算对照 → 复用 TaskDrawer 进度 → 完成刷新)。
  未初始化 → 空态引导"点重训生成权重"。
- `desktop/ui/src/api/ipc.ts`:**新增** `paperRidgeStatus/paperRidgeAdvance/paperRidgeRetrain/paperBlendRecompute`。
- `desktop/ui/src/App.tsx`(非 WIP,干净):MODULES 加 `{key:"paper", label:"纸面盘"}` + `<Route path="/paper">`。

## 数据流 / 错误处理
UI 挂载 → `api.paperRidgeStatus()` → Rust 读三产物 → DTO → 渲染。操作按钮 → `advance/retrain/recompute` →
TaskRegistry → 进度事件 → 完成后重取 status。产物缺失 → `initialized:false` → 空态。Python 退出非 0 →
task error → TaskDrawer 显示 stderr(复用现有)。

## 测试
- Rust:fixture(样例 journal.csv/weights.json/blend.json)→ `paper_cmds` 读取/解析/nav/excess 单测(TDD)。
- UI:vitest 给 mock `PaperStatusDto` 渲染 PaperRidge.tsx(含未初始化态)。
- Python:`eval_blend.py --json` 产物形态测试(键齐全、6 折)。
- 收尾闸:`cargo build`(重生 ts-rs bindings)+ `npm --prefix desktop/ui run build` + `vitest run`。

## 交付 / gm_tail 共存(已定)
- 新文件零冲突;lib.rs/ipc.ts 仅新增行(与 gm_tail 空间分离)。
- **桌面变更暂不提交**——与 gm_tail WIP 一起留工作区,等 gm_tail 落定后由用户统一提交。
- **Python 改动(eval_blend `--json`)单独提交**(无 gm_tail 纠缠)。
- 实现后 `cargo build` + 直接跑 `target/debug/rquant-desktop.exe`(无监视器)给用户看效果。

## 范围外(YAGNI)
- 不做实时双引擎纸面盘(value 腿前向册)——对照用历史回测即可。
- 不做策略上实盘(冻结部署不动)。
- 不改 paper_ridge.py 计算逻辑。
