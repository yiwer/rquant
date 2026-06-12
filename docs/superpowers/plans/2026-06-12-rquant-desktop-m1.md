# rquant 桌面端 M1（骨架 + 驾驶舱）实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 建立 Tauri 2 桌面工作台骨架（workspace 接线 / 桥接层 / 任务系统 / 导航壳）并交付 G 驾驶舱模块（三账本监控 + 手动触发 run）。

**Architecture:** 方案一进程内库调用——`desktop/src-tauri` 桥接 crate 直接依赖根 crate `rquant`，命令层零业务逻辑（DTO 转换 + 任务调度 + 路径解析）；前端 React 18 + TS + Vite + antd + zustand + ECharts。spec：`docs/superpowers/specs/2026-06-12-rquant-desktop-design.md`。

**Tech Stack:** Tauri 2 / React 18 + TypeScript + Vite / Ant Design 5 / zustand / ECharts 5 / ts-rs 10 / vitest。Node v24 + npm 10 已装机。

**分支：** `desktop-m1`（从 master 切出）。

---

## 工程师必读上下文（零仓库背景假设）

**引擎事实（已核对源码，照抄即可）：**

1. 根 crate `rquant`（edition 2024）是库 + CLI；桥接层只调库函数。
2. 驾驶舱消费的引擎类型（`src/signal/mod.rs`）：
   - `PaperState { version: u32, tree_name: String, last_time: Option<NaiveDateTime>, account: AccountSnapshot }`
   - `read_paper_state(path, tree_name) -> Result<Option<PaperState>>`：文件不存在 → `Ok(None)`；空/损坏 → `Err`（消息含 "corrupt"）；version≠1 或 tree_name 不符 → `Err`。
   - `HoldingsState { version, tree_name, last_time, holdings: BTreeMap<String, f64> }`，`read_holdings_state` 同上语义。
   - `SingleSignal { t, target, current_pos, delta, reason, leaf: Option<String>, paper: PaperStats }`；`PaperStats { nav, total_return, max_drawdown, bars_replayed }`。
   - `PortfolioSignal { t, n_fresh, targets: Vec<(String, f64)>, trades: Vec<TradeInstr> }`；`TradeInstr { symbol, action: TradeAction(Buy/Sell/Adjust/Hold), from_w, to_w }`。
   - `pub async fn run_signal_single(cfg: &SignalSingleConfig, llm: &LlmEvaluator) -> Result<(SingleSignal, PaperState)>`——**落盘由调用方决定**（commit 才 `write_paper_state`）。
   - `pub async fn run_signal_portfolio(cfg: &SignalPortfolioConfig, llm: &LlmEvaluator) -> Result<(PortfolioSignal, HoldingsState)>`，commit → `write_holdings_state`。
   - `AccountSnapshot` 13 字段：`pos, entry_price: Option<f64>, bars_held: usize, nav, peak_nav, max_drawdown, turnover, last_increase_date: Option<NaiveDate>, max_price_since_entry/min_price_since_entry/bars_since_exit/last_trip_return: Option<f64>, trip: Option<TripSnapshot>`。
3. 三账本参数是 `deploy/paper_run.cmd` 的镜像（**那是事实源，桥接层 const 注释必须指回它**）：
   - 账本1/2（single）：symbol `sh600030`/`sh600036`，tree `deploy/tree_v4_frozen.yaml`，primary `paper/p_<sym>.csv`，state `paper/paper_<sym>.json`，sig out `paper/sig_<sym>.json`，fetch scale **60** datalen **1023** adjust **qfq**，warmup **80**，window **100**（CLI 默认），cost_bps **10.0**，soft **false**。
   - 账本3（portfolio）：tree `deploy/strength_v1_frozen.yaml`，universe `deploy/universe_10.csv`（10 行 `symbol,primary`，primary 指 `paper/pd_*.csv`），top **3**，soft **true**，warmup 80，window 100，cost 10.0，state `paper/holdings_top3.json`，out `paper/sig_portfolio.json`；run 前按 universe 逐个 fetch scale **240** datalen 1023 qfq（**串行**，防 sina 封禁）。
   - state 校验需要树名：运行时用 `rquant::tree::loader::load_tree_file(&path)?.meta.name` 取（勿硬编码字符串）。
4. 引擎配合改动（仅 spec §4 第 2 项，本期三个符号提升可见性，**不改任何逻辑**）：
   - `src/cli/mod.rs:22` `fn build_llm(model: String, base_url: String, cache_dir: PathBuf) -> anyhow::Result<LlmEvaluator>` → `pub fn`。空 model/base_url = 未配置 LLM（LLM 节点走默认分支），manual run 传 `("".into(), "".into(), ".rquant-cache/llm".into())`。
   - `src/cli/mod.rs:306` `pub(crate) async fn run_fetch_to_csv(symbol: &str, scale: u32, datalen: u32, base_url: &str, adjust: &str, out: &Path) -> anyhow::Result<usize>` → `pub async fn`。
   - `SINA_BASE_URL` const → `pub`（fetch 的 base_url 实参）。
   - `LlmEvaluator` 路径：`rquant::eval::llm::LlmEvaluator`。
5. 纸面盘文件（工作区相对路径）：`paper/run.log`（UTF-8，cmd 已 chcp 65001；段落以 `==== ` 开头行分隔）；schtask 名 `rquant-paper`。
6. **纪律红线**：重放/回测语义冻结——本计划不碰 `src/signal`、`src/backtest` 的任何逻辑行；引擎全量测试是每个任务的回归闸。git 提交永远点名文件（不用 `-A`/通配）；提交信息英文。

**工作区路径约定（桥接层 `paths.rs` 唯一出口）：** 工作区根 = 仓库根（M1 写死为编译期/运行期检测的 app 当前目录的上溯，见 T4）；`.rquant-desktop/`（gitignore）下：`journal/paper-journal.jsonl`。

**验证命令（每任务通用）：**
- 引擎回归：`cargo test`（根包，全量绿）
- 桥接层：`cargo test -p rquant-desktop`
- 前端：`cd desktop/ui && npx tsc --noEmit && npx vitest run`
- 全量 lint：`cargo clippy --workspace --all-targets -- -D warnings`

---

### Task 1: workspace 接线 + 桥接 crate 占位

**Files:**
- Modify: `Cargo.toml`（根）
- Create: `desktop/src-tauri/Cargo.toml`
- Create: `desktop/src-tauri/src/lib.rs`
- Modify: `.gitignore`

- [ ] **Step 1: 切分支**

```bash
git checkout -b desktop-m1
```

- [ ] **Step 2: 根 Cargo.toml 追加 workspace 节**（追加到文件末尾，已有内容不动）

```toml

[workspace]
members = ["desktop/src-tauri"]
```

- [ ] **Step 3: 创建桥接 crate 最小骨架**

`desktop/src-tauri/Cargo.toml`：

```toml
[package]
name = "rquant-desktop"
version = "0.1.0"
edition = "2024"

[lib]
name = "rquant_desktop"

[dependencies]
rquant = { path = "../.." }
serde = { version = "1", features = ["derive"] }
serde_json = { version = "1", features = ["float_roundtrip"] }
chrono = { version = "0.4", features = ["serde"] }
anyhow = "1"

[dev-dependencies]
tempfile = "3"
```

`desktop/src-tauri/src/lib.rs`：

```rust
//! rquant 桌面端桥接层：DTO 转换 + 任务调度 + 工作区路径解析。
//! 零业务逻辑——一切计算调 `rquant` 库；spec: docs/superpowers/specs/2026-06-12-rquant-desktop-design.md
```

- [ ] **Step 4: .gitignore 增加桌面端留档目录**（在 `# 实验临时数据` 节后追加）

```
# 桌面端留档（journal/回测 runs,可再生）
/.rquant-desktop
```

- [ ] **Step 5: 验证 workspace 接线**

Run: `cargo check -p rquant-desktop`
Expected: 编译通过（拉取 rquant 依赖树）。

Run: `cargo test`
Expected: 根包全量测试数与 master 一致（workspace 化不改变根包默认目标），全绿。

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml desktop/src-tauri/Cargo.toml desktop/src-tauri/src/lib.rs .gitignore
git status --porcelain   # 确认无意外文件
git commit -m "feat(desktop): workspace wiring + bridge crate skeleton"
```

---

### Task 2: 前端脚手架 + 导航壳

**Files:**
- Create: `desktop/ui/`（vite react-ts 模板）
- Create/Modify: `desktop/ui/src/App.tsx`、`desktop/ui/src/main.tsx`、`desktop/ui/src/App.test.tsx`、`desktop/ui/vite.config.ts`、`desktop/ui/package.json`

- [ ] **Step 1: 脚手架**

```bash
cd desktop
npm create vite@latest ui -- --template react-ts
cd ui
npm install
npm install react-router-dom antd zustand echarts @tauri-apps/api
npm install -D vitest @testing-library/react @testing-library/jest-dom jsdom
```

- [ ] **Step 2: vite.config.ts**（端口固定 5173 + vitest + @bindings 别名，T5 生成目录先占位）

```ts
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: { "@bindings": path.resolve(__dirname, "../src-tauri/bindings") },
  },
  server: { port: 5173, strictPort: true },
  // vitest
  test: {
    environment: "jsdom",
    globals: true,
  },
} as any);
```

并在 `package.json` 的 scripts 增加 `"test": "vitest run"`。

- [ ] **Step 3: 导航壳 App.tsx**（8 模块侧边栏，驾驶舱为默认路由，其余 7 个占位页）

```tsx
import { HashRouter, Routes, Route, Navigate, useNavigate, useLocation } from "react-router-dom";
import { Layout, Menu, Typography } from "antd";

export const MODULES = [
  { key: "cockpit", label: "驾驶舱" },
  { key: "backtest", label: "回测中心" },
  { key: "data", label: "数据工作台" },
  { key: "tree", label: "策略树" },
  { key: "factor", label: "因子工作台" },
  { key: "wfo", label: "调参/WFO" },
  { key: "portfolio", label: "组合回测" },
  { key: "archive", label: "档案馆" },
];

function Placeholder({ name }: { name: string }) {
  return <Typography.Text type="secondary">{name} —— M2+ 交付</Typography.Text>;
}

function Shell() {
  const nav = useNavigate();
  const loc = useLocation();
  const selected = MODULES.find((m) => loc.pathname.startsWith(`/${m.key}`))?.key ?? "cockpit";
  return (
    <Layout style={{ minHeight: "100vh" }}>
      <Layout.Sider theme="light" width={140}>
        <Menu
          mode="inline"
          selectedKeys={[selected]}
          items={MODULES.map((m) => ({ key: m.key, label: m.label }))}
          onClick={(e) => nav(`/${e.key}`)}
        />
      </Layout.Sider>
      <Layout.Content style={{ padding: 16 }}>
        <Routes>
          <Route path="/cockpit" element={<Placeholder name="驾驶舱" />} />
          {MODULES.filter((m) => m.key !== "cockpit").map((m) => (
            <Route key={m.key} path={`/${m.key}`} element={<Placeholder name={m.label} />} />
          ))}
          <Route path="*" element={<Navigate to="/cockpit" replace />} />
        </Routes>
      </Layout.Content>
    </Layout>
  );
}

export default function App() {
  return (
    <HashRouter>
      <Shell />
    </HashRouter>
  );
}
```

`main.tsx` 保持模板生成形态（render `<App />`），删除模板的 `App.css`/logo 装饰代码。

- [ ] **Step 4: 写壳测试 App.test.tsx**

```tsx
import { render, screen } from "@testing-library/react";
import "@testing-library/jest-dom";
import App, { MODULES } from "./App";

test("sidebar renders all 8 modules and lands on cockpit", () => {
  render(<App />);
  for (const m of MODULES) {
    expect(screen.getByText(m.label)).toBeInTheDocument();
  }
  expect(screen.getByText(/驾驶舱 —— M2\+ 交付|驾驶舱/)).toBeInTheDocument();
});
```

- [ ] **Step 5: 验证**

Run: `cd desktop/ui && npx tsc --noEmit && npx vitest run && npm run build`
Expected: 类型检查过、1 测试绿、`dist/` 产出。

- [ ] **Step 6: Commit**（ui 目录首次入库——vite 模板自带 .gitignore 已排除 node_modules/dist，新文件逐个确认后可用目录路径 add）

```bash
git status --porcelain   # 应只见 desktop/ui/ 源码与配置,无 node_modules/dist
git add desktop/ui
git commit -m "feat(desktop): ui scaffold - react+ts+vite shell with 8-module sidebar"
```

---

### Task 3: Tauri 化（窗口能跑起来）

**Files:**
- Modify: `desktop/src-tauri/Cargo.toml`、`desktop/src-tauri/src/lib.rs`
- Create: `desktop/src-tauri/src/main.rs`、`desktop/src-tauri/build.rs`、`desktop/src-tauri/tauri.conf.json`、`desktop/src-tauri/icons/*`、`desktop/src-tauri/capabilities/default.json`
- Modify: `desktop/ui/package.json`

- [ ] **Step 1: 安装 tauri CLI 并生成图标**

```bash
cd desktop/ui && npm install -D @tauri-apps/cli@^2
# 任意 1024x1024 PNG 作源；用 npx 自带生成器从纯色占位生成全套图标:
cd ../src-tauri && mkdir icons
# 用 PowerShell 生成一张纯色 1024 png 占位（System.Drawing）:
powershell -nop -c "Add-Type -AssemblyName System.Drawing; $b=New-Object Drawing.Bitmap 1024,1024; $g=[Drawing.Graphics]::FromImage($b); $g.Clear([Drawing.Color]::FromArgb(255,30,60,120)); $b.Save('icon-src.png'); $g.Dispose(); $b.Dispose()"
cd ../ui && npx tauri icon ../src-tauri/icon-src.png -o ../src-tauri/icons
```

- [ ] **Step 2: src-tauri 补 Tauri 依赖**（Cargo.toml `[dependencies]` 增加；并加 `[build-dependencies]` 与 bin）

```toml
[dependencies]
# ……既有依赖保持……
tauri = { version = "2", features = [] }
tauri-plugin-log = "2"
tokio = { version = "1", features = ["rt-multi-thread"] }

[build-dependencies]
tauri-build = { version = "2", features = [] }
```

- [ ] **Step 3: tauri.conf.json**

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "rquant-desktop",
  "version": "0.1.0",
  "identifier": "com.rquant.desktop",
  "build": {
    "beforeDevCommand": "npm --prefix ../ui run dev",
    "devUrl": "http://localhost:5173",
    "beforeBuildCommand": "npm --prefix ../ui run build",
    "frontendDist": "../ui/dist"
  },
  "app": {
    "windows": [
      { "title": "rquant 桌面工作台", "width": 1280, "height": 800 }
    ],
    "security": { "csp": null }
  },
  "bundle": { "active": false, "icon": ["icons/icon.ico"] }
}
```

`capabilities/default.json`（Tauri 2 权限面，M1 仅核心 + 事件）：

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "windows": ["main"],
  "permissions": ["core:default"]
}
```

- [ ] **Step 4: build.rs / main.rs / lib.rs 入口**

`build.rs`：

```rust
fn main() {
    tauri_build::build()
}
```

`src/main.rs`：

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    rquant_desktop::run()
}
```

`src/lib.rs` 增加：

```rust
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().build())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 5: 验证**

Run: `cargo build -p rquant-desktop`
Expected: 编译通过（首次拉 tauri 依赖较慢）。

Run（人工冒烟，开发者执行后关窗）: `cd desktop/ui && npx tauri dev`
Expected: 弹出"rquant 桌面工作台"窗口，侧边栏 8 项可点。

- [ ] **Step 6: Commit**

```bash
git add desktop/src-tauri/Cargo.toml desktop/src-tauri/build.rs desktop/src-tauri/src/main.rs desktop/src-tauri/src/lib.rs desktop/src-tauri/tauri.conf.json desktop/src-tauri/capabilities/default.json desktop/src-tauri/icons desktop/ui/package.json desktop/ui/package-lock.json Cargo.lock
git status --porcelain   # icon-src.png 不入库,可删
git commit -m "feat(desktop): tauri 2 app boots with ui shell"
```

---

### Task 4: 错误映射 + 工作区路径（TDD）

**Files:**
- Create: `desktop/src-tauri/src/error.rs`、`desktop/src-tauri/src/paths.rs`
- Modify: `desktop/src-tauri/src/lib.rs`

- [ ] **Step 1: 写失败测试**（先写在各模块 `#[cfg(test)]` 内）

`error.rs` 测试：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rquant_error_maps_kind_and_message() {
        let e = rquant::Error::Data("bad csv".into());
        let dto = ErrorDto::from(&anyhow::Error::new(e));
        assert_eq!(dto.kind, "data");
        assert!(dto.message.contains("bad csv"));
    }

    #[test]
    fn corrupt_state_gets_actionable_advice() {
        let e = rquant::Error::Data("state corrupt: empty file".into());
        let dto = ErrorDto::from(&anyhow::Error::new(e));
        assert!(dto.advice.as_deref().unwrap_or("").contains("删除"));
    }

    #[test]
    fn non_rquant_error_is_internal() {
        let dto = ErrorDto::from(&anyhow::anyhow!("boom"));
        assert_eq!(dto.kind, "internal");
    }
}
```

`paths.rs` 测试：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_paths_join_correctly() {
        let ws = Workspace::new(std::path::PathBuf::from("E:/rust-app/rquant"));
        assert!(ws.paper_dir().ends_with("paper"));
        assert!(ws.deploy_dir().ends_with("deploy"));
        assert!(ws.journal_path().ends_with(".rquant-desktop/journal/paper-journal.jsonl")
            || ws.journal_path().ends_with(".rquant-desktop\\journal\\paper-journal.jsonl"));
    }

    #[test]
    fn detect_workspace_walks_up_to_cargo_toml_with_paper_run() {
        // detect 规则:从给定起点向上找同时含 Cargo.toml 与 deploy/paper_run.cmd 的目录
        let here = std::env::current_dir().unwrap();
        let ws = Workspace::detect(&here).expect("repo root should be detectable from src-tauri cwd");
        assert!(ws.root().join("deploy").join("paper_run.cmd").exists());
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rquant-desktop`
Expected: 编译错误（模块不存在）——即"失败"形态。

- [ ] **Step 3: 实现**

`src/error.rs`：

```rust
//! 引擎/任意错误 → 前端 DTO 映射。kind 与 rquant::Error 九类一一对应。
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ErrorDto {
    pub kind: String,
    pub message: String,
    /// 可操作建议（如 state corrupt → 删除重建,重放幂等）。
    pub advice: Option<String>,
}

impl ErrorDto {
    pub fn from(e: &anyhow::Error) -> Self {
        let (kind, message) = match e.downcast_ref::<rquant::Error>() {
            Some(re) => {
                let k = match re {
                    rquant::Error::Data(_) => "data",
                    rquant::Error::Dsl(_) => "dsl",
                    rquant::Error::Tree(_) => "tree",
                    rquant::Error::Eval(_) => "eval",
                    rquant::Error::Engine(_) => "engine",
                    rquant::Error::Backtest(_) => "backtest",
                    rquant::Error::Io(_) => "io",
                    rquant::Error::Csv(_) => "csv",
                    rquant::Error::Yaml(_) => "yaml",
                    rquant::Error::Json(_) => "json",
                };
                (k.to_string(), re.to_string())
            }
            None => ("internal".to_string(), e.to_string()),
        };
        let advice = if message.contains("corrupt") {
            Some("state 文件损坏:可删除该 state 后重新运行(重放幂等,会从头重建账本)".to_string())
        } else if message.contains("tree_name") || message.contains("串树") {
            Some("state 与树不匹配:确认账本对应的冻结树未被改名".to_string())
        } else {
            None
        };
        ErrorDto { kind, message, advice }
    }
}
```

> 注意:`rquant::Error` 的变体数以编译错误为准——若上面列举少了变体,补全 match 即可(禁止 `_ =>` 兜底,保持新增变体时编译期提醒)。

`src/paths.rs`：

```rust
//! 工作区路径唯一出口——桥接层任何文件访问都经此模块取路径。
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    pub fn new(root: PathBuf) -> Self {
        Workspace { root }
    }

    /// 自 start 向上找仓库根:同时存在 Cargo.toml 与 deploy/paper_run.cmd 的目录。
    pub fn detect(start: &Path) -> Option<Self> {
        let mut cur = Some(start);
        while let Some(d) = cur {
            if d.join("Cargo.toml").exists() && d.join("deploy").join("paper_run.cmd").exists() {
                return Some(Workspace::new(d.to_path_buf()));
            }
            cur = d.parent();
        }
        None
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn paper_dir(&self) -> PathBuf {
        self.root.join("paper")
    }
    pub fn deploy_dir(&self) -> PathBuf {
        self.root.join("deploy")
    }
    pub fn desktop_data_dir(&self) -> PathBuf {
        self.root.join(".rquant-desktop")
    }
    pub fn journal_path(&self) -> PathBuf {
        self.desktop_data_dir().join("journal").join("paper-journal.jsonl")
    }
    pub fn run_log_path(&self) -> PathBuf {
        self.paper_dir().join("run.log")
    }
}
```

`lib.rs` 增加模块声明：

```rust
pub mod error;
pub mod paths;
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p rquant-desktop`
Expected: 5 测试全绿。

- [ ] **Step 5: Commit**

```bash
git add desktop/src-tauri/src/error.rs desktop/src-tauri/src/paths.rs desktop/src-tauri/src/lib.rs
git commit -m "feat(desktop): error dto mapping + workspace path resolution"
```

---

### Task 5: DTO 定义 + ts-rs 类型生成

**Files:**
- Create: `desktop/src-tauri/src/dto.rs`
- Modify: `desktop/src-tauri/Cargo.toml`、`desktop/src-tauri/src/lib.rs`
- Create（生成物，入库）: `desktop/src-tauri/bindings/*.ts`
- Modify: `desktop/ui/tsconfig.app.json`

- [ ] **Step 1: 加依赖**（src-tauri Cargo.toml `[dependencies]`）

```toml
ts-rs = { version = "10", features = ["serde-compat", "chrono-impl"] }
```

- [ ] **Step 2: 写 dto.rs（全部驾驶舱 DTO 一次定义清楚——后续任务的函数签名以此为准）**

```rust
//! 前端 DTO——桥接层对外的唯一数据形态;全部派生 ts-rs 供 ui 生成 TS 类型。
//! 字段语义对照 spec §5.1;时间一律 ISO-8601 字符串(前端不解析 NaiveDateTime)。
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct BookCardDto {
    /// "b1" | "b2" | "b3"
    pub book: String,
    pub title: String,
    /// "single" | "portfolio"
    pub kind: String,
    /// "ok" | "empty"(state 未建) | "corrupt"(state 损坏/串树)
    pub status: String,
    /// status != ok 时的可操作建议。
    pub advice: Option<String>,
    /// 以下来自已 commit 的 state(empty/corrupt 时 None)。
    pub nav: Option<f64>,
    pub total_return: Option<f64>,
    pub max_drawdown: Option<f64>,
    pub pos: Option<f64>,
    pub state_time: Option<String>,
    /// 账本3:当前持仓清单(symbol, weight)。
    pub holdings: Option<Vec<(String, f64)>>,
    /// 最新信号(来自 sig_*.json;时间戳可能比 state 新——dry 残留,如实分开展示)。
    pub last_signal: Option<SignalBriefDto>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SignalBriefDto {
    pub t: String,
    /// single:目标仓位;portfolio:入选数。
    pub target: Option<f64>,
    pub current_pos: Option<f64>,
    pub delta: Option<f64>,
    pub reason: Option<String>,
    pub leaf: Option<String>,
    pub bars_replayed: Option<u64>,
    /// portfolio:目标清单。
    pub targets: Option<Vec<(String, f64)>>,
    pub n_fresh: Option<u64>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DiffRowDto {
    pub symbol: String,
    /// "Buy" | "Sell" | "Adjust" | "Hold"
    pub action: String,
    pub from_w: f64,
    pub to_w: f64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RunlogStatusDto {
    /// 最近一段 run 的头行(==== 日期 ====)。
    pub last_header: Option<String>,
    /// true=最近段含 committed/DRY 正常收尾;false=可疑(含 error 等);None=无日志。
    pub ok: Option<bool>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SchtaskDto {
    pub next_run: Option<String>,
    pub last_run: Option<String>,
    pub last_result: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct OverviewDto {
    pub cards: Vec<BookCardDto>,
    /// 账本3 今日清单 diff(来自 sig_portfolio.json trades)。
    pub diff: Vec<DiffRowDto>,
    pub diff_t: Option<String>,
    pub runlog: RunlogStatusDto,
    /// schtasks 查询失败/任务不存在 → None。
    pub schtask: Option<SchtaskDto>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SnapshotDto {
    pub pos: f64,
    pub entry_price: Option<f64>,
    pub bars_held: u64,
    pub nav: f64,
    pub peak_nav: f64,
    pub max_drawdown: f64,
    pub turnover: f64,
    pub last_increase_date: Option<String>,
    pub max_price_since_entry: Option<f64>,
    pub min_price_since_entry: Option<f64>,
    pub bars_since_exit: Option<f64>,
    pub last_trip_return: Option<f64>,
    /// TripSnapshot 原样 JSON(UI 直接展示,不拆字段)。
    #[ts(type = "unknown")]
    pub trip: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct JournalPointDto {
    pub state_time: String,
    pub nav: Option<f64>,
    pub pos: Option<f64>,
    /// 账本3:成员数。
    pub members: Option<u32>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct BookDetailDto {
    pub card: BookCardDto,
    pub snapshot: Option<SnapshotDto>,
    pub journal: Vec<JournalPointDto>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TaskProgressDto {
    pub pct: f32,
    pub stage: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TaskInfoDto {
    pub id: String,
    pub kind: String,
    /// "running" | "done" | "failed" | "cancelled"
    pub status: String,
    pub progress: TaskProgressDto,
    pub error: Option<String>,
    /// 完成结果(JSON 任意形态,manual_run 放 run 摘要)。
    #[ts(type = "unknown")]
    pub result: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct GateDto {
    /// "allow" | "dry_only" | "warn"
    pub gate: String,
    pub message: Option<String>,
}
```

`lib.rs` 增加 `pub mod dto;`。

- [ ] **Step 3: 生成 TS 类型并接到 ui**

Run: `cargo test -p rquant-desktop export_bindings`
Expected: ts-rs 导出测试绿，`desktop/src-tauri/bindings/` 出现 `BookCardDto.ts` 等文件。

`desktop/ui/tsconfig.app.json` 的 `compilerOptions` 增加：

```json
"paths": { "@bindings/*": ["../src-tauri/bindings/*"] }
```

- [ ] **Step 4: ui 侧消费一个类型验证链路**——`desktop/ui/src/api/types.test.ts`：

```ts
import type { BookCardDto } from "@bindings/BookCardDto";

test("bindings types are importable", () => {
  const card: BookCardDto = {
    book: "b1", title: "t", kind: "single", status: "empty",
    advice: null, nav: null, total_return: null, max_drawdown: null,
    pos: null, state_time: null, holdings: null, last_signal: null,
  };
  expect(card.book).toBe("b1");
});
```

Run: `cd desktop/ui && npx tsc --noEmit && npx vitest run`
Expected: 全绿（字段对不上会在 tsc 阶段爆——这就是类型链路的回归闸）。

- [ ] **Step 5: Commit**

```bash
git add desktop/src-tauri/Cargo.toml desktop/src-tauri/src/dto.rs desktop/src-tauri/src/lib.rs desktop/src-tauri/bindings desktop/ui/tsconfig.app.json desktop/ui/src/api/types.test.ts Cargo.lock
git commit -m "feat(desktop): cockpit DTOs with ts-rs generated bindings"
```

---

### Task 6: TaskRegistry（TDD：启动/进度/取消/panic/重任务独占）

**Files:**
- Create: `desktop/src-tauri/src/tasks.rs`
- Modify: `desktop/src-tauri/src/lib.rs`

- [ ] **Step 1: 写失败测试**（tasks.rs 底部 `#[cfg(test)]`；轮询等待用 helper）

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    struct NullSink;
    impl ProgressSink for NullSink {
        fn emit(&self, _info: &crate::dto::TaskInfoDto) {}
    }

    fn wait_status(reg: &TaskRegistry, id: &str, want: &str) -> crate::dto::TaskInfoDto {
        for _ in 0..200 {
            let info = reg.get(id).unwrap();
            if info.status == want {
                return info;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("task {} never reached {}", id, want);
    }

    fn reg() -> TaskRegistry {
        TaskRegistry::new(Arc::new(NullSink))
    }

    #[test]
    fn task_runs_to_done_with_result() {
        let r = reg();
        let id = r
            .start("demo", false, |ctx| {
                ctx.progress(0.5, "half", "");
                Ok(serde_json::json!({"answer": 42}))
            })
            .unwrap();
        let info = wait_status(&r, &id, "done");
        assert_eq!(info.result.unwrap()["answer"], 42);
    }

    #[test]
    fn cancel_flag_reaches_task_body() {
        let r = reg();
        let id = r
            .start("loop", false, |ctx| {
                for _ in 0..1000 {
                    if ctx.cancelled() {
                        return Err("cancelled by user".into());
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                Ok(serde_json::Value::Null)
            })
            .unwrap();
        std::thread::sleep(Duration::from_millis(30));
        r.cancel(&id);
        let info = wait_status(&r, &id, "cancelled");
        assert!(info.error.unwrap().contains("cancelled"));
    }

    #[test]
    fn panic_becomes_failed_not_process_death() {
        let r = reg();
        let id = r.start("boom", false, |_ctx| panic!("kaboom")).unwrap();
        let info = wait_status(&r, &id, "failed");
        assert!(info.error.unwrap().contains("panic"));
    }

    #[test]
    fn heavy_slot_is_exclusive() {
        let r = reg();
        let _id1 = r
            .start("heavy1", true, |_ctx| {
                std::thread::sleep(Duration::from_millis(300));
                Ok(serde_json::Value::Null)
            })
            .unwrap();
        std::thread::sleep(Duration::from_millis(30));
        let err = r.start("heavy2", true, |_ctx| Ok(serde_json::Value::Null));
        assert!(err.is_err(), "second heavy task must be rejected while first runs");
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rquant-desktop tasks`
Expected: 编译失败（类型未定义）。

- [ ] **Step 3: 实现 tasks.rs**

```rust
//! 任务注册表:长任务统一入口——std::thread + catch_unwind,进度经 ProgressSink 推送。
//! 重任务(网格/批量/manual run)独占一个槽位(spec §12.5);轻命令不经此处。
//! paper/ 写互斥说明:M1 唯一写者 manual_run 是重任务,独占槽位即满足 spec §7 的
//! "同一时刻至多一个 commit 型任务"——后续里程碑引入第二类写者时再升级为显式锁。
use crate::dto::{TaskInfoDto, TaskProgressDto};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub trait ProgressSink: Send + Sync + 'static {
    fn emit(&self, info: &TaskInfoDto);
}

pub struct TaskCtx {
    cancel: Arc<AtomicBool>,
    id: String,
    shared: Arc<Shared>,
}

impl TaskCtx {
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
    pub fn progress(&self, pct: f32, stage: &str, detail: &str) {
        self.shared.update(&self.id, |info| {
            info.progress = TaskProgressDto { pct, stage: stage.to_string(), detail: detail.to_string() };
        });
    }
}

struct Shared {
    tasks: Mutex<HashMap<String, (TaskInfoDto, Arc<AtomicBool>)>>,
    sink: Arc<dyn ProgressSink>,
    heavy_busy: AtomicBool,
}

impl Shared {
    fn update(&self, id: &str, f: impl FnOnce(&mut TaskInfoDto)) {
        let mut g = self.tasks.lock().expect("task map poisoned");
        if let Some((info, _)) = g.get_mut(id) {
            f(info);
            self.sink.emit(info);
        }
    }
}

pub struct TaskRegistry {
    shared: Arc<Shared>,
    seq: AtomicU64,
}

impl TaskRegistry {
    pub fn new(sink: Arc<dyn ProgressSink>) -> Self {
        TaskRegistry {
            shared: Arc::new(Shared { tasks: Mutex::new(HashMap::new()), sink, heavy_busy: AtomicBool::new(false) }),
            seq: AtomicU64::new(1),
        }
    }

    /// heavy=true 时独占重任务槽;占用中返回 Err。
    /// body 返回 Ok(result) → done;Err(含 "cancelled") → cancelled;其余 Err → failed。
    pub fn start<F>(&self, kind: &str, heavy: bool, body: F) -> Result<String, String>
    where
        F: FnOnce(&TaskCtx) -> Result<serde_json::Value, String> + Send + 'static,
    {
        if heavy
            && self
                .shared
                .heavy_busy
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
        {
            return Err("已有重任务运行中,请等待其完成或取消".to_string());
        }
        let id = format!("t{}", self.seq.fetch_add(1, Ordering::Relaxed));
        let cancel = Arc::new(AtomicBool::new(false));
        let info = TaskInfoDto {
            id: id.clone(),
            kind: kind.to_string(),
            status: "running".to_string(),
            progress: TaskProgressDto { pct: 0.0, stage: "start".into(), detail: String::new() },
            error: None,
            result: None,
        };
        {
            let mut g = self.shared.tasks.lock().expect("task map poisoned");
            g.insert(id.clone(), (info.clone(), cancel.clone()));
        }
        self.shared.sink.emit(&info);

        let ctx = TaskCtx { cancel, id: id.clone(), shared: self.shared.clone() };
        let shared = self.shared.clone();
        let tid = id.clone();
        std::thread::spawn(move || {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| body(&ctx)));
            if heavy {
                shared.heavy_busy.store(false, Ordering::SeqCst);
            }
            shared.update(&tid, |info| match &outcome {
                Ok(Ok(v)) => {
                    info.status = "done".into();
                    info.progress.pct = 1.0;
                    info.result = Some(v.clone());
                }
                Ok(Err(msg)) if msg.contains("cancelled") => {
                    info.status = "cancelled".into();
                    info.error = Some(msg.clone());
                }
                Ok(Err(msg)) => {
                    info.status = "failed".into();
                    info.error = Some(msg.clone());
                }
                Err(_) => {
                    info.status = "failed".into();
                    info.error = Some("panic in task body (engine call guarded by catch_unwind)".into());
                }
            });
        });
        Ok(id)
    }

    pub fn cancel(&self, id: &str) {
        let g = self.shared.tasks.lock().expect("task map poisoned");
        if let Some((_, c)) = g.get(id) {
            c.store(true, Ordering::Relaxed);
        }
    }

    pub fn get(&self, id: &str) -> Option<TaskInfoDto> {
        self.shared.tasks.lock().expect("task map poisoned").get(id).map(|(i, _)| i.clone())
    }

    pub fn list(&self) -> Vec<TaskInfoDto> {
        let mut v: Vec<_> = self
            .shared
            .tasks
            .lock()
            .expect("task map poisoned")
            .values()
            .map(|(i, _)| i.clone())
            .collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    }
}
```

`lib.rs` 增加 `pub mod tasks;`。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p rquant-desktop tasks`
Expected: 5 测试全绿（heavy 独占、panic 不带崩进程是重点）。

- [ ] **Step 5: Commit**

```bash
git add desktop/src-tauri/src/tasks.rs desktop/src-tauri/src/lib.rs
git commit -m "feat(desktop): task registry - progress/cancel/panic-guard/heavy-slot"
```

---

### Task 7: 三账本声明 + 状态/信号读取器（TDD，fixture 用引擎结构体构造）

**Files:**
- Create: `desktop/src-tauri/src/books.rs`、`desktop/src-tauri/src/readers.rs`
- Modify: `desktop/src-tauri/src/lib.rs`

- [ ] **Step 1: books.rs（纯声明——deploy/paper_run.cmd 的镜像）**

```rust
//! 三账本静态声明。事实源是 deploy/paper_run.cmd——改那边必须同步这里。
//! 参数核对(2026-06-12):scale 60/240,datalen 1023,qfq,warmup 80,window 100(CLI 默认),
//! cost 10bps,b3 top3 soft 周一 commit。
use crate::paths::Workspace;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookKind {
    Single,
    Portfolio,
}

#[derive(Debug, Clone)]
pub struct Book {
    pub id: &'static str,
    pub title: &'static str,
    pub kind: BookKind,
    /// single:标的;portfolio:空。
    pub symbol: &'static str,
    pub tree_rel: &'static str,
    pub state_rel: &'static str,
    pub sig_rel: &'static str,
    /// fetch 周期(分钟,240=日线)。
    pub scale: u32,
}

pub const BOOKS: [Book; 3] = [
    Book { id: "b1", title: "账本1 · sh600030 60m", kind: BookKind::Single, symbol: "sh600030",
        tree_rel: "deploy/tree_v4_frozen.yaml", state_rel: "paper/paper_sh600030.json",
        sig_rel: "paper/sig_sh600030.json", scale: 60 },
    Book { id: "b2", title: "账本2 · sh600036 60m", kind: BookKind::Single, symbol: "sh600036",
        tree_rel: "deploy/tree_v4_frozen.yaml", state_rel: "paper/paper_sh600036.json",
        sig_rel: "paper/sig_sh600036.json", scale: 60 },
    Book { id: "b3", title: "账本3 · 组合 top3 日线", kind: BookKind::Portfolio, symbol: "",
        tree_rel: "deploy/strength_v1_frozen.yaml", state_rel: "paper/holdings_top3.json",
        sig_rel: "paper/sig_portfolio.json", scale: 240 },
];

impl Book {
    pub fn state_path(&self, ws: &Workspace) -> PathBuf {
        ws.root().join(self.state_rel)
    }
    pub fn sig_path(&self, ws: &Workspace) -> PathBuf {
        ws.root().join(self.sig_rel)
    }
    pub fn tree_path(&self, ws: &Workspace) -> PathBuf {
        ws.root().join(self.tree_rel)
    }
    pub fn primary_csv(&self, ws: &Workspace) -> PathBuf {
        ws.paper_dir().join(format!("p_{}.csv", self.symbol))
    }
}

pub fn find_book(id: &str) -> Option<&'static Book> {
    BOOKS.iter().find(|b| b.id == id)
}
```

- [ ] **Step 2: 写 readers 失败测试**（readers.rs `#[cfg(test)]`；**fixture 一律用引擎结构体构造再 serde 落盘**——结构漂移时编译期就报）

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::books::BOOKS;
    use crate::paths::Workspace;
    use chrono::NaiveDateTime;
    use rquant::backtest::sim::SimAccount;
    use rquant::signal::{write_paper_state, PaperState};

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    /// tempdir 工作区 + 最小 deploy 树副本(读卡要靠树名校验 state)。
    fn fixture_ws() -> (tempfile::TempDir, Workspace) {
        let td = tempfile::tempdir().unwrap();
        let root = td.path().to_path_buf();
        std::fs::create_dir_all(root.join("paper")).unwrap();
        std::fs::create_dir_all(root.join("deploy")).unwrap();
        // 真树文件直接拷贝(测试在仓库内跑,引用真实 deploy 树保证 meta.name 同步)
        let repo = Workspace::detect(&std::env::current_dir().unwrap()).unwrap();
        for f in ["tree_v4_frozen.yaml", "strength_v1_frozen.yaml"] {
            std::fs::copy(repo.deploy_dir().join(f), root.join("deploy").join(f)).unwrap();
        }
        (td, Workspace::new(root))
    }

    #[test]
    fn empty_state_yields_empty_card() {
        let (_td, ws) = fixture_ws();
        let card = read_book_card(&ws, &BOOKS[0]);
        assert_eq!(card.status, "empty");
        assert!(card.nav.is_none());
    }

    #[test]
    fn committed_state_yields_ok_card_with_nav() {
        let (_td, ws) = fixture_ws();
        let tree = rquant::tree::loader::load_tree_file(&BOOKS[0].tree_path(&ws)).unwrap();
        let mut acc = SimAccount::default();
        acc.nav = 1.0539;
        let st = PaperState {
            version: 1,
            tree_name: tree.meta.name.clone(),
            last_time: Some(t("2026-06-11 15:00:00")),
            account: acc.snapshot(),
        };
        write_paper_state(&BOOKS[0].state_path(&ws), &st).unwrap();
        let card = read_book_card(&ws, &BOOKS[0]);
        assert_eq!(card.status, "ok");
        assert!((card.nav.unwrap() - 1.0539).abs() < 1e-12);
        assert_eq!(card.state_time.as_deref(), Some("2026-06-11T15:00:00"));
    }

    #[test]
    fn corrupt_state_yields_corrupt_card_with_advice() {
        let (_td, ws) = fixture_ws();
        std::fs::write(BOOKS[0].state_path(&ws), b"").unwrap(); // 空文件 = corrupt
        let card = read_book_card(&ws, &BOOKS[0]);
        assert_eq!(card.status, "corrupt");
        assert!(card.advice.is_some());
    }

    #[test]
    fn sig_json_feeds_last_signal_even_without_state() {
        let (_td, ws) = fixture_ws();
        // 真实形状:用引擎 SingleSignal 序列化的字段名(t/target/current_pos/delta/reason/leaf/paper)
        let sig = serde_json::json!({
            "t": "2026-06-12T15:00:00", "target": 0.0, "current_pos": 0.0, "delta": 0.0,
            "reason": "tree", "leaf": "flat_wait",
            "paper": {"nav": 1.05, "total_return": 0.05, "max_drawdown": 0.02, "bars_replayed": 942}
        });
        std::fs::write(BOOKS[0].sig_path(&ws), serde_json::to_string(&sig).unwrap()).unwrap();
        let card = read_book_card(&ws, &BOOKS[0]);
        assert_eq!(card.status, "empty"); // state 仍未建
        let brief = card.last_signal.unwrap();
        assert_eq!(brief.bars_replayed, Some(942));
        assert_eq!(brief.leaf.as_deref(), Some("flat_wait"));
    }

    #[test]
    fn portfolio_card_and_diff_rows() {
        let (_td, ws) = fixture_ws();
        let b3 = &BOOKS[2];
        let tree = rquant::tree::loader::load_tree_file(&b3.tree_path(&ws)).unwrap();
        let mut holdings = std::collections::BTreeMap::new();
        holdings.insert("sh600900".to_string(), 0.5);
        holdings.insert("sz000333".to_string(), 0.5);
        let st = rquant::signal::HoldingsState {
            version: 1,
            tree_name: tree.meta.name.clone(),
            last_time: Some(t("2026-06-11 15:00:00")),
            holdings,
        };
        rquant::signal::write_holdings_state(&b3.state_path(&ws), &st).unwrap();
        let sig = serde_json::json!({
            "t": "2026-06-12T15:00:00", "n_fresh": 10,
            "targets": [["sh600900", 0.5], ["sz000333", 0.5]],
            "trades": [
                {"symbol": "sh600900", "action": "Hold", "from_w": 0.5, "to_w": 0.5},
                {"symbol": "sz000333", "action": "Hold", "from_w": 0.5, "to_w": 0.5}
            ]
        });
        std::fs::write(b3.sig_path(&ws), serde_json::to_string(&sig).unwrap()).unwrap();
        let card = read_book_card(&ws, b3);
        assert_eq!(card.status, "ok");
        assert_eq!(card.holdings.as_ref().unwrap().len(), 2);
        let (rows, t_opt) = read_portfolio_diff(&ws, b3);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].action, "Hold");
        assert_eq!(t_opt.as_deref(), Some("2026-06-12T15:00:00"));
    }

    #[test]
    fn snapshot_dto_mirrors_all_13_fields() {
        let acc = SimAccount::default();
        let snap = acc.snapshot();
        let dto = snapshot_to_dto(&snap);
        assert_eq!(dto.pos, 0.0);
        assert!(dto.entry_price.is_none()); // default NaN → None
    }
}
```

- [ ] **Step 3: 跑测试确认失败**

Run: `cargo test -p rquant-desktop readers`
Expected: 编译失败（readers 模块不存在）。

- [ ] **Step 4: 实现 readers.rs**

```rust
//! 账本卡片/diff/快照读取——全只读,引擎零改动(spec §5.1)。
//! 设计要点:state 与 sig 各有自己的时间戳,可能不一致(dry 残留),如实分开返回。
use crate::books::{Book, BookKind};
use crate::dto::{BookCardDto, DiffRowDto, SignalBriefDto, SnapshotDto};
use crate::paths::Workspace;
use rquant::backtest::sim::AccountSnapshot;
use rquant::signal::{read_holdings_state, read_paper_state, PortfolioSignal, SingleSignal};

fn iso(t: &chrono::NaiveDateTime) -> String {
    t.format("%Y-%m-%dT%H:%M:%S").to_string()
}

pub fn snapshot_to_dto(s: &AccountSnapshot) -> SnapshotDto {
    SnapshotDto {
        pos: s.pos,
        entry_price: s.entry_price,
        bars_held: s.bars_held as u64,
        nav: s.nav,
        peak_nav: s.peak_nav,
        max_drawdown: s.max_drawdown,
        turnover: s.turnover,
        last_increase_date: s.last_increase_date.map(|d| d.to_string()),
        max_price_since_entry: s.max_price_since_entry,
        min_price_since_entry: s.min_price_since_entry,
        bars_since_exit: s.bars_since_exit,
        last_trip_return: s.last_trip_return,
        trip: s.trip.as_ref().map(|t| serde_json::to_value(t).unwrap_or(serde_json::Value::Null)),
    }
}

fn read_single_sig(path: &std::path::Path) -> Option<SignalBriefDto> {
    let txt = std::fs::read_to_string(path).ok()?;
    let sig: SingleSignal = serde_json::from_str(&txt).ok()?;
    Some(SignalBriefDto {
        t: iso(&sig.t),
        target: Some(sig.target),
        current_pos: Some(sig.current_pos),
        delta: Some(sig.delta),
        reason: Some(sig.reason),
        leaf: sig.leaf,
        bars_replayed: Some(sig.paper.bars_replayed as u64),
        targets: None,
        n_fresh: None,
    })
}

fn read_portfolio_sig(path: &std::path::Path) -> Option<(SignalBriefDto, Vec<DiffRowDto>)> {
    let txt = std::fs::read_to_string(path).ok()?;
    let sig: PortfolioSignal = serde_json::from_str(&txt).ok()?;
    let brief = SignalBriefDto {
        t: iso(&sig.t),
        target: None,
        current_pos: None,
        delta: None,
        reason: None,
        leaf: None,
        bars_replayed: None,
        targets: Some(sig.targets.clone()),
        n_fresh: Some(sig.n_fresh as u64),
    };
    let rows = sig
        .trades
        .iter()
        .map(|tr| DiffRowDto {
            symbol: tr.symbol.clone(),
            action: format!("{:?}", tr.action),
            from_w: tr.from_w,
            to_w: tr.to_w,
        })
        .collect();
    Some((brief, rows))
}

/// 树名取自冻结树文件 meta(勿硬编码);树文件本身坏了也归为 corrupt 卡。
fn tree_name(ws: &Workspace, book: &Book) -> Result<String, String> {
    rquant::tree::loader::load_tree_file(&book.tree_path(ws))
        .map(|t| t.meta.name)
        .map_err(|e| e.to_string())
}

pub fn read_book_card(ws: &Workspace, book: &Book) -> BookCardDto {
    let mut card = BookCardDto {
        book: book.id.to_string(),
        title: book.title.to_string(),
        kind: match book.kind { BookKind::Single => "single", BookKind::Portfolio => "portfolio" }.to_string(),
        status: "empty".to_string(),
        advice: None,
        nav: None,
        total_return: None,
        max_drawdown: None,
        pos: None,
        state_time: None,
        holdings: None,
        last_signal: None,
    };

    match book.kind {
        BookKind::Single => {
            card.last_signal = read_single_sig(&book.sig_path(ws));
            match tree_name(ws, book).map(|n| read_paper_state(&book.state_path(ws), &n)) {
                Ok(Ok(Some(st))) => {
                    card.status = "ok".into();
                    card.nav = Some(st.account.nav);
                    card.total_return = Some(st.account.nav - 1.0);
                    card.max_drawdown = Some(st.account.max_drawdown);
                    card.pos = Some(st.account.pos);
                    card.state_time = st.last_time.as_ref().map(iso);
                }
                Ok(Ok(None)) => {
                    card.advice = Some("state 未建账:等待 15:35 schtask 首跑,或手动触发 run(收盘后)".into());
                }
                Ok(Err(e)) | Err(e) => {
                    let e_str = e.to_string();
                    card.status = "corrupt".into();
                    card.advice = Some(crate::error::ErrorDto::from(&anyhow::anyhow!(e_str)).advice
                        .unwrap_or_else(|| "state 异常:查看消息并考虑删除重建(重放幂等)".into()));
                }
            }
        }
        BookKind::Portfolio => {
            if let Some((brief, _)) = read_portfolio_sig(&book.sig_path(ws)) {
                card.last_signal = Some(brief);
            }
            match tree_name(ws, book).map(|n| read_holdings_state(&book.state_path(ws), &n)) {
                Ok(Ok(Some(st))) => {
                    card.status = "ok".into();
                    card.holdings = Some(st.holdings.iter().map(|(s, w)| (s.clone(), *w)).collect());
                    card.state_time = st.last_time.as_ref().map(iso);
                }
                Ok(Ok(None)) => {
                    card.advice = Some("holdings 未建账:首次 commit 在周一 15:35(周频 reb5)".into());
                }
                Ok(Err(e)) | Err(e) => {
                    let e_str = e.to_string();
                    card.status = "corrupt".into();
                    card.advice = Some("holdings state 异常:".to_string() + &e_str);
                }
            }
        }
    }
    card
}

/// 账本3 今日清单 diff——直接采 sig_portfolio.json 的 trades(引擎已算好)。
pub fn read_portfolio_diff(ws: &Workspace, book: &Book) -> (Vec<DiffRowDto>, Option<String>) {
    match read_portfolio_sig(&book.sig_path(ws)) {
        Some((brief, rows)) => (rows, Some(brief.t)),
        None => (Vec::new(), None),
    }
}
```

`lib.rs` 增加 `pub mod books; pub mod readers;`。

> 若 `SingleSignal`/`PortfolioSignal`/`HoldingsState`/`write_holdings_state` 未在 `rquant::signal` 重导出（编译报私有），属于 spec §4 第 2 项可见性提升——在 `src/signal/mod.rs` 将其改为 `pub` 并跑引擎全量测试，提交信息记 `feat(signal): pub visibility for desktop bridge`。**只改可见性，不改逻辑。**

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test -p rquant-desktop readers && cargo test`
Expected: 桥接 6 测试绿 + 引擎全量绿（若做了可见性提升）。

- [ ] **Step 6: Commit**

```bash
git add desktop/src-tauri/src/books.rs desktop/src-tauri/src/readers.rs desktop/src-tauri/src/lib.rs
git status --porcelain   # 若引擎可见性提升,补 git add src/signal/mod.rs
git commit -m "feat(desktop): book declarations + state/signal readers with engine-struct fixtures"
```

---

### Task 8: 纸面 journal（TDD：append 去重 / 读取序列）

**Files:**
- Create: `desktop/src-tauri/src/journal.rs`
- Modify: `desktop/src-tauri/src/lib.rs`

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Workspace;

    fn ws() -> (tempfile::TempDir, Workspace) {
        let td = tempfile::tempdir().unwrap();
        (td, Workspace::new(td.path().to_path_buf()))
    }

    fn entry(book: &str, t: &str, nav: f64) -> JournalEntry {
        JournalEntry {
            appended_at: "2026-06-12T16:00:00".into(),
            book: book.into(),
            state_time: t.into(),
            nav: Some(nav),
            pos: Some(1.0),
            members: None,
        }
    }

    #[test]
    fn append_dedups_by_book_and_state_time() {
        let (_td, w) = ws();
        append_entries(&w, &[entry("b1", "2026-06-11T15:00:00", 1.01)]).unwrap();
        append_entries(&w, &[entry("b1", "2026-06-11T15:00:00", 1.01)]).unwrap(); // 重复
        append_entries(&w, &[entry("b1", "2026-06-12T15:00:00", 1.02)]).unwrap();
        let pts = read_points(&w, "b1").unwrap();
        assert_eq!(pts.len(), 2);
        assert_eq!(pts[0].state_time, "2026-06-11T15:00:00");
        assert_eq!(pts[1].state_time, "2026-06-12T15:00:00");
    }

    #[test]
    fn books_are_isolated() {
        let (_td, w) = ws();
        append_entries(&w, &[entry("b1", "2026-06-11T15:00:00", 1.01)]).unwrap();
        append_entries(&w, &[entry("b2", "2026-06-11T15:00:00", 0.99)]).unwrap();
        assert_eq!(read_points(&w, "b1").unwrap().len(), 1);
        assert_eq!(read_points(&w, "b2").unwrap().len(), 1);
    }

    #[test]
    fn atomic_rewrite_keeps_file_valid_jsonl() {
        let (_td, w) = ws();
        append_entries(&w, &[entry("b1", "2026-06-11T15:00:00", 1.01)]).unwrap();
        let txt = std::fs::read_to_string(w.journal_path()).unwrap();
        for line in txt.lines() {
            serde_json::from_str::<JournalEntry>(line).expect("every line valid json");
        }
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rquant-desktop journal`
Expected: 编译失败。

- [ ] **Step 3: 实现 journal.rs**

```rust
//! 纸面盘净值 journal——桌面端自建历史(spec §5.1:state 只有最新快照)。
//! jsonl 一行一条;读全量→去重→temp+rename 整体重写(文件量级:年数百行,无性能问题)。
use crate::dto::JournalPointDto;
use crate::paths::Workspace;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub appended_at: String,
    /// "b1" | "b2" | "b3"
    pub book: String,
    /// 去重键的一半:已 commit state 的 last_time(ISO)。
    pub state_time: String,
    pub nav: Option<f64>,
    pub pos: Option<f64>,
    /// 账本3:持仓成员数。
    pub members: Option<u32>,
}

fn read_all(ws: &Workspace) -> Vec<JournalEntry> {
    let Ok(txt) = std::fs::read_to_string(ws.journal_path()) else {
        return Vec::new();
    };
    txt.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

/// 追加条目(按 (book, state_time) 去重,保持原有顺序,新条目排尾)。
pub fn append_entries(ws: &Workspace, new: &[JournalEntry]) -> anyhow::Result<()> {
    let mut all = read_all(ws);
    let mut seen: BTreeSet<(String, String)> =
        all.iter().map(|e| (e.book.clone(), e.state_time.clone())).collect();
    let mut changed = false;
    for e in new {
        if seen.insert((e.book.clone(), e.state_time.clone())) {
            all.push(e.clone());
            changed = true;
        }
    }
    if !changed {
        return Ok(());
    }
    let path = ws.journal_path();
    std::fs::create_dir_all(path.parent().expect("journal path has parent"))?;
    let mut buf = String::new();
    for e in &all {
        buf.push_str(&serde_json::to_string(e)?);
        buf.push('\n');
    }
    let tmp = path.with_extension("jsonl.tmp");
    std::fs::write(&tmp, &buf)?;
    std::fs::rename(&tmp, &path)?; // 原子替换(spec §7)
    Ok(())
}

/// 某账本的净值序列(按 state_time 升序)。
pub fn read_points(ws: &Workspace, book: &str) -> anyhow::Result<Vec<JournalPointDto>> {
    let mut pts: Vec<JournalPointDto> = read_all(ws)
        .into_iter()
        .filter(|e| e.book == book)
        .map(|e| JournalPointDto { state_time: e.state_time, nav: e.nav, pos: e.pos, members: e.members })
        .collect();
    pts.sort_by(|a, b| a.state_time.cmp(&b.state_time));
    Ok(pts)
}
```

`lib.rs` 增加 `pub mod journal;`。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p rquant-desktop journal`
Expected: 3 测试绿。

- [ ] **Step 5: Commit**

```bash
git add desktop/src-tauri/src/journal.rs desktop/src-tauri/src/lib.rs
git commit -m "feat(desktop): paper nav journal - dedup append + atomic rewrite"
```

---

### Task 9: run.log 段落解析 + schtasks 查询（TDD，字符串 fixture）

**Files:**
- Create: `desktop/src-tauri/src/runlog.rs`、`desktop/src-tauri/src/schtask.rs`
- Modify: `desktop/src-tauri/src/lib.rs`

- [ ] **Step 1: 写失败测试**

`runlog.rs` 测试（fixture 取自真实 run.log 形态）：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "\
==== Thu 06/11/2026 15:35:00.10 ====
fetched 1023 bars for sh600030
=== rquant SIGNAL (single) @ 2026-06-11 15:00:00 ===
committed state to paper\\paper_sh600030.json
==== Fri 06/12/2026 14:14:34.12 ====
fetched 1023 bars for sh600030
=== rquant SIGNAL (single) @ 2026-06-12 15:00:00 ===
[DRY RUN] 未落盘 state；加 --commit 提交
";

    #[test]
    fn splits_sections_by_marker_and_takes_latest() {
        let st = classify(LOG);
        assert_eq!(st.last_header.as_deref(), Some("==== Fri 06/12/2026 14:14:34.12 ===="));
        assert_eq!(st.ok, Some(true)); // DRY 收尾也算正常
    }

    #[test]
    fn error_section_flags_not_ok() {
        let log = "==== Fri 06/12/2026 15:35:00.00 ====\nerror: data error: bad csv\n";
        let st = classify(log);
        assert_eq!(st.ok, Some(false));
        assert!(st.summary.contains("error"));
    }

    #[test]
    fn empty_log_is_none() {
        let st = classify("");
        assert_eq!(st.ok, None);
    }

    #[test]
    fn tail_returns_last_n_lines() {
        let t = tail_lines(LOG, 2);
        assert_eq!(t.lines().count(), 2);
        assert!(t.contains("DRY RUN"));
    }
}
```

`schtask.rs` 测试（fixture = `schtasks /query /tn rquant-paper /fo csv /v` 的 CSV 头+行裁剪）：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const CSV: &str = "\
\"HostName\",\"TaskName\",\"Next Run Time\",\"Status\",\"Last Run Time\",\"Last Result\"
\"HOST\",\"\\rquant-paper\",\"6/12/2026 3:35:00 PM\",\"Ready\",\"11/30/1999 12:00:00 AM\",\"267011\"
";

    #[test]
    fn parses_columns_by_header_name() {
        let dto = parse_schtasks_csv(CSV).unwrap();
        assert_eq!(dto.next_run.as_deref(), Some("6/12/2026 3:35:00 PM"));
        assert_eq!(dto.status.as_deref(), Some("Ready"));
        assert_eq!(dto.last_result.as_deref(), Some("267011"));
    }

    #[test]
    fn garbage_returns_none() {
        assert!(parse_schtasks_csv("not,a,real,header\n").is_none());
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rquant-desktop runlog schtask`
Expected: 编译失败。

- [ ] **Step 3: 实现**

`src/runlog.rs`：

```rust
//! run.log 解析:段落以 "==== " 开头行分隔(deploy/paper_run.cmd 的 echo 格式)。
use crate::dto::RunlogStatusDto;
use crate::paths::Workspace;

pub fn classify(log: &str) -> RunlogStatusDto {
    let mut sections: Vec<(String, Vec<&str>)> = Vec::new();
    for line in log.lines() {
        if line.starts_with("==== ") {
            sections.push((line.to_string(), Vec::new()));
        } else if let Some((_, body)) = sections.last_mut() {
            body.push(line);
        }
    }
    let Some((header, body)) = sections.last() else {
        return RunlogStatusDto { last_header: None, ok: None, summary: "run.log 为空或不存在".into() };
    };
    let text = body.join("\n");
    let lower = text.to_lowercase();
    let bad = lower.contains("error") || lower.contains("panic");
    let finished = text.contains("committed state") || text.contains("[DRY RUN]");
    let ok = !bad && finished;
    let summary = if bad {
        format!("最近一次 run 含错误行:{}", body.iter().find(|l| l.to_lowercase().contains("error")).unwrap_or(&""))
    } else if finished {
        "最近一次 run 正常收尾".to_string()
    } else {
        "最近一次 run 无收尾标记(可能中断)".to_string()
    };
    RunlogStatusDto { last_header: Some(header.clone()), ok: Some(ok), summary }
}

pub fn tail_lines(log: &str, n: usize) -> String {
    let lines: Vec<&str> = log.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

pub fn read_status(ws: &Workspace) -> RunlogStatusDto {
    let log = std::fs::read_to_string(ws.run_log_path()).unwrap_or_default();
    classify(&log)
}

pub fn read_tail(ws: &Workspace, n: usize) -> String {
    let log = std::fs::read_to_string(ws.run_log_path()).unwrap_or_default();
    tail_lines(&log, n)
}
```

`src/schtask.rs`：

```rust
//! schtasks /query 包装——任务缺失/解析失败一律 None(驾驶舱降级显示)。
use crate::dto::SchtaskDto;

pub fn parse_schtasks_csv(csv_text: &str) -> Option<SchtaskDto> {
    let mut rdr = csv::Reader::from_reader(csv_text.as_bytes());
    let headers = rdr.headers().ok()?.clone();
    let find = |name: &str| headers.iter().position(|h| h == name);
    let (i_next, i_status, i_last, i_res) =
        (find("Next Run Time")?, find("Status")?, find("Last Run Time")?, find("Last Result")?);
    let rec = rdr.records().next()?.ok()?;
    let get = |i: usize| rec.get(i).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    Some(SchtaskDto { next_run: get(i_next), last_run: get(i_last), last_result: get(i_res), status: get(i_status) })
}

/// 实时查询(测试不调用;commands 层用)。
pub fn query(task_name: &str) -> Option<SchtaskDto> {
    let out = std::process::Command::new("schtasks")
        .args(["/query", "/tn", task_name, "/fo", "csv", "/v"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_schtasks_csv(&String::from_utf8_lossy(&out.stdout))
}
```

src-tauri Cargo.toml `[dependencies]` 增加 `csv = "1"`。`lib.rs` 增加 `pub mod runlog; pub mod schtask;`。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p rquant-desktop runlog schtask`
Expected: 6 测试绿。

- [ ] **Step 5: Commit**

```bash
git add desktop/src-tauri/src/runlog.rs desktop/src-tauri/src/schtask.rs desktop/src-tauri/src/lib.rs desktop/src-tauri/Cargo.toml Cargo.lock
git commit -m "feat(desktop): run.log section classifier + schtasks csv probe"
```

---

### Task 10: 运行纪律闸（TDD）+ 引擎 glue 可见性提升

**Files:**
- Create: `desktop/src-tauri/src/gates.rs`
- Modify: `desktop/src-tauri/src/lib.rs`、`src/cli/mod.rs`（仅可见性）

- [ ] **Step 1: 写失败测试**（表驱动覆盖边界；规则=spec §5.1：工作日 [09:30,15:00) 禁 commit；[15:30,15:40) 警告；其余放行）

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
    }

    #[test]
    fn gate_table() {
        // 2026-06-12 = 周五;06-13 = 周六
        let cases = [
            ("2026-06-12 09:29", "allow"),
            ("2026-06-12 09:30", "dry_only"),
            ("2026-06-12 11:00", "dry_only"),
            ("2026-06-12 14:59", "dry_only"),
            ("2026-06-12 15:00", "allow"),
            ("2026-06-12 15:29", "allow"),
            ("2026-06-12 15:30", "warn"),
            ("2026-06-12 15:39", "warn"),
            ("2026-06-12 15:40", "allow"),
            ("2026-06-13 11:00", "allow"), // 周六盘中时刻也放行(无成形 bar 风险)
        ];
        for (when, want) in cases {
            let g = classify_run_window(t(when));
            assert_eq!(g.gate, want, "at {}", when);
        }
    }

    #[test]
    fn messages_explain_why() {
        let g = classify_run_window(t("2026-06-12 11:00"));
        assert!(g.message.unwrap().contains("forming"));
        let g = classify_run_window(t("2026-06-12 15:35"));
        assert!(g.message.unwrap().contains("schtask"));
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rquant-desktop gates`
Expected: 编译失败。

- [ ] **Step 3: 实现 gates.rs**

```rust
//! 手动 run 的纪律闸(spec §5.1)。纯函数,时钟由调用方注入——可测。
use crate::dto::GateDto;
use chrono::{Datelike, NaiveDateTime, NaiveTime, Weekday};

pub fn classify_run_window(now: NaiveDateTime) -> GateDto {
    let wd = now.weekday();
    let weekday = !matches!(wd, Weekday::Sat | Weekday::Sun);
    let t = now.time();
    let hm = |h, m| NaiveTime::from_hms_opt(h, m, 0).expect("valid literal time");
    if weekday && t >= hm(9, 30) && t < hm(15, 0) {
        return GateDto {
            gate: "dry_only".into(),
            message: Some("盘中:sina 末根为 forming bar,commit 会以未定型价格记账——仅允许 DRY".into()),
        };
    }
    if weekday && t >= hm(15, 30) && t < hm(15, 40) {
        return GateDto {
            gate: "warn".into(),
            message: Some("与 15:35 schtask 窗口重叠:并发 commit 有竞态风险(幂等可兜底),确认后继续".into()),
        };
    }
    GateDto { gate: "allow".into(), message: None }
}
```

`lib.rs` 增加 `pub mod gates;`。

- [ ] **Step 4: 引擎 glue 可见性提升**（`src/cli/mod.rs`，三处，**仅改可见性关键字**）

- 第 22 行 `fn build_llm(...)` → `pub fn build_llm(...)`
- 第 306 行 `pub(crate) async fn run_fetch_to_csv(...)` → `pub async fn run_fetch_to_csv(...)`
- `SINA_BASE_URL` 常量 → `pub const`（若已 pub 跳过）

各加一行文档注释注明 `桌面端桥接层复用(spec §4-2)`。

- [ ] **Step 5: 验证**

Run: `cargo test -p rquant-desktop gates && cargo test && cargo clippy --workspace --all-targets -- -D warnings`
Expected: gates 2 测试绿；引擎全量绿；clippy 干净（pub fn 未用警告若出现，在桥接层 T11 使用后消失——本步可容忍 dead_code 提示则改用 `cargo clippy -p rquant`）。

- [ ] **Step 6: Commit**

```bash
git add desktop/src-tauri/src/gates.rs desktop/src-tauri/src/lib.rs src/cli/mod.rs
git commit -m "feat(desktop): run-window discipline gates; pub visibility for cli glue (spec 4-2)"
```

---

### Task 11: 命令装配（cockpit_overview / book_detail / manual_run / task_*）

**Files:**
- Create: `desktop/src-tauri/src/commands.rs`、`desktop/src-tauri/src/manual_run.rs`
- Modify: `desktop/src-tauri/src/lib.rs`

- [ ] **Step 1: 写失败测试**（commands 的纯装配函数可直测——tauri 宏包的薄壳不测）

`manual_run.rs` 测试聚焦参数构造（不真跑网络/重放）：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::books::BOOKS;
    use crate::paths::Workspace;

    #[test]
    fn single_cfg_mirrors_paper_run_cmd() {
        let ws = Workspace::new(std::path::PathBuf::from("E:/x"));
        let cfg = single_cfg(&ws, &BOOKS[0]);
        assert_eq!(cfg.warmup, 80);
        assert_eq!(cfg.window, 100);
        assert!((cfg.cost_bps - 10.0).abs() < 1e-12);
        assert!(!cfg.soft);
        assert!(cfg.primary_path.ends_with("paper/p_sh600030.csv") || cfg.primary_path.ends_with("paper\\p_sh600030.csv"));
        assert_eq!(cfg.context_path, cfg.primary_path); // cmd 未传 --context → primary
        assert!(cfg.news_path.is_none());
        assert!(cfg.aux_paths.is_empty());
    }

    #[test]
    fn portfolio_cfg_mirrors_paper_run_cmd() {
        let ws = Workspace::new(std::path::PathBuf::from("E:/x"));
        let cfg = portfolio_cfg(&ws);
        assert_eq!(cfg.top, 3);
        assert!(cfg.soft);
        assert_eq!(cfg.warmup, 80);
        assert!(cfg.universe_path.ends_with("deploy/universe_10.csv") || cfg.universe_path.ends_with("deploy\\universe_10.csv"));
    }

    #[test]
    fn universe_symbols_parse() {
        let repo = Workspace::detect(&std::env::current_dir().unwrap()).unwrap();
        let syms = universe_symbols(&repo).unwrap();
        assert_eq!(syms.len(), 10);
        assert!(syms.contains(&"sh600519".to_string()));
    }
}
```

`commands.rs` 测试（overview 装配函数直测，复用 T7 的 fixture 思路）：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Workspace;

    #[test]
    fn overview_assembles_three_cards_and_appends_journal() {
        let td = tempfile::tempdir().unwrap();
        let root = td.path().to_path_buf();
        std::fs::create_dir_all(root.join("paper")).unwrap();
        std::fs::create_dir_all(root.join("deploy")).unwrap();
        let repo = Workspace::detect(&std::env::current_dir().unwrap()).unwrap();
        for f in ["tree_v4_frozen.yaml", "strength_v1_frozen.yaml"] {
            std::fs::copy(repo.deploy_dir().join(f), root.join("deploy").join(f)).unwrap();
        }
        let ws = Workspace::new(root);
        let dto = assemble_overview(&ws);
        assert_eq!(dto.cards.len(), 3);
        assert_eq!(dto.cards[0].book, "b1");
        // 全 empty → journal 不应产生文件
        assert!(!ws.journal_path().exists());
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rquant-desktop commands manual_run`
Expected: 编译失败。

- [ ] **Step 3: 实现 manual_run.rs**

```rust
//! 手动触发当日 run——参数严格镜像 deploy/paper_run.cmd(事实源,books.rs 同注)。
//! 在任务线程内自建 tokio runtime 跑引擎 async 函数。
use crate::books::{Book, BookKind, BOOKS};
use crate::paths::Workspace;
use crate::tasks::TaskCtx;
use rquant::signal::{SignalPortfolioConfig, SignalSingleConfig};

pub fn single_cfg(ws: &Workspace, book: &Book) -> SignalSingleConfig {
    let primary = book.primary_csv(ws);
    SignalSingleConfig {
        tree_path: book.tree_path(ws),
        primary_path: primary.clone(),
        context_path: primary,
        news_path: None,
        aux_paths: Vec::new(),
        window: 100,
        warmup: 80,
        cost_bps: 10.0,
        soft: false,
        state_path: book.state_path(ws),
    }
}

pub fn portfolio_cfg(ws: &Workspace) -> SignalPortfolioConfig {
    let b3 = &BOOKS[2];
    SignalPortfolioConfig {
        tree_path: b3.tree_path(ws),
        universe_path: ws.deploy_dir().join("universe_10.csv"),
        top: 3,
        window: 100,
        warmup: 80,
        cost_bps: 10.0,
        soft: true,
        aux_paths: Vec::new(),
        state_path: b3.state_path(ws),
    }
}

pub fn universe_symbols(ws: &Workspace) -> anyhow::Result<Vec<String>> {
    let txt = std::fs::read_to_string(ws.deploy_dir().join("universe_10.csv"))?;
    Ok(txt
        .lines()
        .skip(1)
        .filter_map(|l| l.split(',').next())
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string())
        .collect())
}

/// 任务体:books 子集 + commit 旗标。返回 run 摘要 JSON。
pub fn run_books(ws: &Workspace, ctx: &TaskCtx, book_ids: &[String], commit: bool) -> Result<serde_json::Value, String> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let llm = rquant::cli::build_llm(String::new(), String::new(), ".rquant-cache/llm".into())
        .map_err(|e| e.to_string())?;
    let mut summary = Vec::new();
    let total = book_ids.len() as f32;

    for (i, id) in book_ids.iter().enumerate() {
        if ctx.cancelled() {
            return Err("cancelled by user".into());
        }
        let base = i as f32 / total;
        let book = crate::books::find_book(id).ok_or_else(|| format!("unknown book {}", id))?;
        match book.kind {
            BookKind::Single => {
                ctx.progress(base + 0.1 / total, "fetch", book.symbol);
                rt.block_on(rquant::cli::run_fetch_to_csv(
                    book.symbol, book.scale, 1023, rquant::cli::SINA_BASE_URL, "qfq", &book.primary_csv(ws),
                ))
                .map_err(|e| e.to_string())?;
                ctx.progress(base + 0.5 / total, "replay", book.symbol);
                let cfg = single_cfg(ws, book);
                let (sig, new_state) =
                    rt.block_on(rquant::signal::run_signal_single(&cfg, &llm)).map_err(|e| e.to_string())?;
                std::fs::write(book.sig_path(ws), serde_json::to_string_pretty(&sig).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
                if commit {
                    rquant::signal::write_paper_state(&cfg.state_path, &new_state).map_err(|e| e.to_string())?;
                }
                summary.push(serde_json::json!({
                    "book": book.id, "t": sig.t.to_string(), "target": sig.target,
                    "bars_replayed": sig.paper.bars_replayed, "committed": commit
                }));
            }
            BookKind::Portfolio => {
                let syms = universe_symbols(ws).map_err(|e| e.to_string())?;
                for (j, s) in syms.iter().enumerate() {
                    if ctx.cancelled() {
                        return Err("cancelled by user".into());
                    }
                    ctx.progress(base + (0.6 * j as f32 / syms.len() as f32) / total, "fetch", s);
                    rt.block_on(rquant::cli::run_fetch_to_csv(
                        s, 240, 1023, rquant::cli::SINA_BASE_URL, "qfq",
                        &ws.paper_dir().join(format!("pd_{}.csv", s)),
                    ))
                    .map_err(|e| e.to_string())?;
                    std::thread::sleep(std::time::Duration::from_millis(500)); // sina 节流
                }
                ctx.progress(base + 0.8 / total, "select", "top3");
                let cfg = portfolio_cfg(ws);
                let (sig, new_state) =
                    rt.block_on(rquant::signal::run_signal_portfolio(&cfg, &llm)).map_err(|e| e.to_string())?;
                std::fs::write(BOOKS[2].sig_path(ws), serde_json::to_string_pretty(&sig).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
                if commit {
                    rquant::signal::write_holdings_state(&cfg.state_path, &new_state).map_err(|e| e.to_string())?;
                }
                summary.push(serde_json::json!({
                    "book": "b3", "t": sig.t.to_string(), "n_fresh": sig.n_fresh,
                    "targets": sig.targets, "committed": commit
                }));
            }
        }
    }
    Ok(serde_json::Value::Array(summary))
}
```

- [ ] **Step 4: 实现 commands.rs（纯装配 + tauri 薄壳）**

```rust
//! Tauri 命令层:薄壳——装配函数可直测,#[tauri::command] 仅做提取与转发。
use crate::books::{find_book, BOOKS};
use crate::dto::*;
use crate::journal::{append_entries, read_points, JournalEntry};
use crate::paths::Workspace;
use crate::readers::{read_book_card, read_portfolio_diff, snapshot_to_dto};
use crate::tasks::TaskRegistry;
use std::sync::Arc;

pub struct AppState {
    pub ws: Workspace,
    pub tasks: Arc<TaskRegistry>,
}

pub fn assemble_overview(ws: &Workspace) -> OverviewDto {
    let cards: Vec<BookCardDto> = BOOKS.iter().map(|b| read_book_card(ws, b)).collect();
    let (diff, diff_t) = read_portfolio_diff(ws, &BOOKS[2]);
    // journal 顺带 append(仅 status=ok 的卡;幂等去重)
    let now = chrono::Local::now().naive_local().format("%Y-%m-%dT%H:%M:%S").to_string();
    let entries: Vec<JournalEntry> = cards
        .iter()
        .filter(|c| c.status == "ok")
        .filter_map(|c| {
            c.state_time.clone().map(|st| JournalEntry {
                appended_at: now.clone(),
                book: c.book.clone(),
                state_time: st,
                nav: c.nav,
                pos: c.pos,
                members: c.holdings.as_ref().map(|h| h.len() as u32),
            })
        })
        .collect();
    if !entries.is_empty() {
        let _ = append_entries(ws, &entries); // journal 失败不阻断 overview(降级)
    }
    OverviewDto {
        cards,
        diff,
        diff_t,
        runlog: crate::runlog::read_status(ws),
        schtask: crate::schtask::query("rquant-paper"),
    }
}

pub fn assemble_book_detail(ws: &Workspace, book_id: &str) -> Result<BookDetailDto, String> {
    let book = find_book(book_id).ok_or_else(|| format!("unknown book {}", book_id))?;
    let card = read_book_card(ws, book);
    // 13 字段快照:仅 single 且 state ok
    let snapshot = if card.kind == "single" && card.status == "ok" {
        let name = rquant::tree::loader::load_tree_file(&book.tree_path(ws)).map_err(|e| e.to_string())?.meta.name;
        rquant::signal::read_paper_state(&book.state_path(ws), &name)
            .ok()
            .flatten()
            .map(|st| snapshot_to_dto(&st.account))
    } else {
        None
    };
    let journal = read_points(ws, book_id).unwrap_or_default();
    Ok(BookDetailDto { card, snapshot, journal })
}

// ---- tauri 薄壳 ----

#[tauri::command]
pub fn cockpit_overview(state: tauri::State<AppState>) -> OverviewDto {
    assemble_overview(&state.ws)
}

#[tauri::command]
pub fn book_detail(state: tauri::State<AppState>, book: String) -> Result<BookDetailDto, String> {
    assemble_book_detail(&state.ws, &book)
}

#[tauri::command]
pub fn runlog_tail(state: tauri::State<AppState>, lines: usize) -> String {
    crate::runlog::read_tail(&state.ws, lines)
}

#[tauri::command]
pub fn run_gate_now() -> GateDto {
    crate::gates::classify_run_window(chrono::Local::now().naive_local())
}

/// commit 时闸校验:dry_only 拒绝;warn 需 confirmed=true。
#[tauri::command]
pub fn manual_run(
    state: tauri::State<AppState>,
    books: Vec<String>,
    commit: bool,
    confirmed: bool,
) -> Result<String, String> {
    if commit {
        let gate = crate::gates::classify_run_window(chrono::Local::now().naive_local());
        match gate.gate.as_str() {
            "dry_only" => return Err(gate.message.unwrap_or_else(|| "盘中禁 commit".into())),
            "warn" if !confirmed => return Err(format!("CONFIRM:{}", gate.message.unwrap_or_default())),
            _ => {}
        }
    }
    let ws = state.ws.clone();
    state
        .tasks
        .start("manual_run", true, move |ctx| crate::manual_run::run_books(&ws, ctx, &books, commit))
}

#[tauri::command]
pub fn task_list(state: tauri::State<AppState>) -> Vec<TaskInfoDto> {
    state.tasks.list()
}

#[tauri::command]
pub fn task_cancel(state: tauri::State<AppState>, id: String) {
    state.tasks.cancel(&id)
}
```

`lib.rs` 的 `run()` 改为注册 state 与命令（进度 sink 用 tauri 事件）：

```rust
pub mod books;
pub mod commands;
pub mod dto;
pub mod error;
pub mod gates;
pub mod journal;
pub mod manual_run;
pub mod paths;
pub mod readers;
pub mod runlog;
pub mod schtask;
pub mod tasks;

use std::sync::Arc;
use tauri::Emitter;

struct TauriSink(tauri::AppHandle);
impl tasks::ProgressSink for TauriSink {
    fn emit(&self, info: &dto::TaskInfoDto) {
        let _ = self.0.emit(&format!("task://progress/{}", info.id), info);
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().build())
        .setup(|app| {
            use tauri::Manager;
            let ws = paths::Workspace::detect(&std::env::current_dir()?)
                .ok_or("workspace not found: run from inside the rquant repo")?;
            let sink = Arc::new(TauriSink(app.handle().clone()));
            app.manage(commands::AppState { ws, tasks: Arc::new(tasks::TaskRegistry::new(sink)) });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::cockpit_overview,
            commands::book_detail,
            commands::runlog_tail,
            commands::run_gate_now,
            commands::manual_run,
            commands::task_list,
            commands::task_cancel,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 5: 验证**

Run: `cargo test -p rquant-desktop && cargo test && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 桥接层全部测试绿（含新增 4 个）；引擎全量绿；clippy 干净。

- [ ] **Step 6: Commit**

```bash
git add desktop/src-tauri/src/commands.rs desktop/src-tauri/src/manual_run.rs desktop/src-tauri/src/lib.rs
git commit -m "feat(desktop): cockpit commands - overview/detail/manual-run with gates and journal"
```

---

### Task 12: UI 驾驶舱总览页

**Files:**
- Create: `desktop/ui/src/api/ipc.ts`、`desktop/ui/src/stores/cockpit.ts`、`desktop/ui/src/pages/Cockpit.tsx`、`desktop/ui/src/components/BookCard.tsx`、`desktop/ui/src/components/DiffTable.tsx`、`desktop/ui/src/components/RunStatusPanel.tsx`、`desktop/ui/src/components/ManualRunButton.tsx`
- Create: `desktop/ui/src/pages/Cockpit.test.tsx`
- Modify: `desktop/ui/src/App.tsx`

- [ ] **Step 1: ipc.ts（typed invoke 包装；测试环境可注入 mock）**

```ts
import { invoke } from "@tauri-apps/api/core";
import type { OverviewDto } from "@bindings/OverviewDto";
import type { BookDetailDto } from "@bindings/BookDetailDto";
import type { GateDto } from "@bindings/GateDto";
import type { TaskInfoDto } from "@bindings/TaskInfoDto";

export const api = {
  cockpitOverview: () => invoke<OverviewDto>("cockpit_overview"),
  bookDetail: (book: string) => invoke<BookDetailDto>("book_detail", { book }),
  runlogTail: (lines: number) => invoke<string>("runlog_tail", { lines }),
  runGateNow: () => invoke<GateDto>("run_gate_now"),
  manualRun: (books: string[], commit: boolean, confirmed: boolean) =>
    invoke<string>("manual_run", { books, commit, confirmed }),
  taskList: () => invoke<TaskInfoDto[]>("task_list"),
  taskCancel: (id: string) => invoke<void>("task_cancel", { id }),
};
export type Api = typeof api;
```

- [ ] **Step 2: stores/cockpit.ts（zustand；api 可替换用于测试）**

```ts
import { create } from "zustand";
import type { OverviewDto } from "@bindings/OverviewDto";
import { api as realApi, type Api } from "../api/ipc";

interface CockpitState {
  api: Api;
  overview: OverviewDto | null;
  loading: boolean;
  error: string | null;
  load: () => Promise<void>;
}

export const useCockpit = create<CockpitState>((set, get) => ({
  api: realApi,
  overview: null,
  loading: false,
  error: null,
  load: async () => {
    set({ loading: true, error: null });
    try {
      set({ overview: await get().api.cockpitOverview(), loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },
}));
```

- [ ] **Step 3: 组件**

`components/BookCard.tsx`：

```tsx
import { Card, Statistic, Tag, Typography } from "antd";
import type { BookCardDto } from "@bindings/BookCardDto";
import { useNavigate } from "react-router-dom";

const STATUS_TAG: Record<string, { color: string; text: string }> = {
  ok: { color: "green", text: "正常" },
  empty: { color: "default", text: "未建账" },
  corrupt: { color: "red", text: "异常" },
};

export default function BookCard({ card }: { card: BookCardDto }) {
  const nav = useNavigate();
  const st = STATUS_TAG[card.status] ?? STATUS_TAG.empty;
  return (
    <Card
      size="small"
      title={card.title}
      extra={<Tag color={st.color}>{st.text}</Tag>}
      hoverable
      onClick={() => nav(`/cockpit/${card.book}`)}
      style={{ flex: 1, minWidth: 260 }}
    >
      {card.status === "ok" && card.kind === "single" && (
        <>
          <Statistic title="nav" value={card.nav ?? 0} precision={4} />
          <Typography.Text type="secondary">
            持仓 {card.pos} · 回撤 {((card.max_drawdown ?? 0) * 100).toFixed(2)}% · {card.state_time}
          </Typography.Text>
        </>
      )}
      {card.status === "ok" && card.kind === "portfolio" && (
        <Typography.Text>
          持仓 {card.holdings?.map(([s, w]) => `${s} ${w.toFixed(2)}`).join(" / ") || "(空)"}
        </Typography.Text>
      )}
      {card.status !== "ok" && <Typography.Text type="secondary">{card.advice}</Typography.Text>}
      {card.last_signal && (
        <div style={{ marginTop: 8 }}>
          <Typography.Text type="secondary" style={{ fontSize: 12 }}>
            最新信号 {card.last_signal.t}
            {card.last_signal.leaf ? ` · 叶 ${card.last_signal.leaf}` : ""}
            {card.last_signal.targets ? ` · 入选 ${card.last_signal.targets.length} 只` : ""}
            {card.last_signal.bars_replayed != null ? ` · 重放 ${card.last_signal.bars_replayed}` : ""}
          </Typography.Text>
        </div>
      )}
    </Card>
  );
}
```

`components/DiffTable.tsx`：

```tsx
import { Card, Table, Tag } from "antd";
import type { DiffRowDto } from "@bindings/DiffRowDto";

const ACTION_COLOR: Record<string, string> = { Buy: "green", Sell: "red", Adjust: "orange", Hold: "default" };

export default function DiffTable({ rows, t }: { rows: DiffRowDto[]; t: string | null }) {
  return (
    <Card size="small" title={`今日组合清单 diff${t ? ` @ ${t}` : ""}`}>
      <Table
        size="small"
        rowKey="symbol"
        pagination={false}
        dataSource={rows}
        locale={{ emptyText: "暂无清单(等待账本3 run)" }}
        columns={[
          { title: "标的", dataIndex: "symbol" },
          {
            title: "动作",
            dataIndex: "action",
            render: (a: string) => <Tag color={ACTION_COLOR[a] ?? "default"}>{a}</Tag>,
          },
          { title: "现权重", dataIndex: "from_w", render: (v: number) => v.toFixed(2) },
          { title: "目标权重", dataIndex: "to_w", render: (v: number) => v.toFixed(2) },
        ]}
      />
    </Card>
  );
}
```

`components/RunStatusPanel.tsx`：

```tsx
import { Badge, Card, Typography } from "antd";
import type { RunlogStatusDto } from "@bindings/RunlogStatusDto";
import type { SchtaskDto } from "@bindings/SchtaskDto";

export default function RunStatusPanel({
  runlog,
  schtask,
  onOpenLog,
}: {
  runlog: RunlogStatusDto;
  schtask: SchtaskDto | null;
  onOpenLog: () => void;
}) {
  const status = runlog.ok == null ? "default" : runlog.ok ? "success" : "error";
  return (
    <Card size="small" title="运行状态">
      <Badge status={status as never} text={runlog.summary} />
      <div>
        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
          {runlog.last_header ?? "暂无 run 记录"}
        </Typography.Text>
      </div>
      <div style={{ marginTop: 8 }}>
        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
          schtask: {schtask ? `${schtask.status ?? "?"} · 下次 ${schtask.next_run ?? "?"}` : "未检测到 rquant-paper"}
        </Typography.Text>
      </div>
      <a onClick={onOpenLog}>查看 run.log</a>
    </Card>
  );
}
```

`components/ManualRunButton.tsx`（闸三态交互：dry_only 禁 commit、warn 需确认、CONFIRM: 前缀复确）：

```tsx
import { App as AntApp, Button, Checkbox, Modal, Space } from "antd";
import { useState } from "react";
import { api } from "../api/ipc";

export default function ManualRunButton({ onStarted }: { onStarted: (taskId: string) => void }) {
  const { message, modal } = AntApp.useApp();
  const [open, setOpen] = useState(false);
  const [commit, setCommit] = useState(false);
  const [gateMsg, setGateMsg] = useState<string | null>(null);
  const [dryOnly, setDryOnly] = useState(false);

  const openDialog = async () => {
    const gate = await api.runGateNow();
    setDryOnly(gate.gate === "dry_only");
    setGateMsg(gate.message ?? null);
    setCommit(false);
    setOpen(true);
  };

  const start = async (confirmed: boolean) => {
    try {
      const id = await api.manualRun(["b1", "b2", "b3"], commit, confirmed);
      setOpen(false);
      message.success(`run 已启动(任务 ${id})`);
      onStarted(id);
    } catch (e) {
      const s = String(e);
      if (s.includes("CONFIRM:")) {
        modal.confirm({
          title: "确认在 schtask 窗口附近 commit?",
          content: s.replace(/^.*CONFIRM:/, ""),
          okText: "确认执行",
          onOk: () => start(true),
        });
      } else {
        message.error(s);
      }
    }
  };

  return (
    <>
      <Button type="primary" onClick={openDialog}>手动触发 run</Button>
      <Modal title="手动触发当日 run" open={open} onCancel={() => setOpen(false)} onOk={() => start(false)} okText="运行">
        <Space direction="vertical">
          <span>账本:b1 + b2 + b3(参数与 deploy/paper_run.cmd 一致)</span>
          {gateMsg && <span style={{ color: dryOnly ? "#cf1322" : "#d48806" }}>{gateMsg}</span>}
          <Checkbox checked={commit} disabled={dryOnly} onChange={(e) => setCommit(e.target.checked)}>
            commit(落盘 state;不勾 = DRY RUN)
          </Checkbox>
        </Space>
      </Modal>
    </>
  );
}
```

- [ ] **Step 4: pages/Cockpit.tsx + 路由接线**

```tsx
import { useEffect, useState } from "react";
import { Alert, Col, Drawer, Row, Spin, Typography } from "antd";
import { useCockpit } from "../stores/cockpit";
import BookCard from "../components/BookCard";
import DiffTable from "../components/DiffTable";
import RunStatusPanel from "../components/RunStatusPanel";
import ManualRunButton from "../components/ManualRunButton";
import { api } from "../api/ipc";

export default function Cockpit() {
  const { overview, loading, error, load } = useCockpit();
  const [logOpen, setLogOpen] = useState(false);
  const [logText, setLogText] = useState("");

  useEffect(() => {
    void load();
  }, [load]);

  const openLog = async () => {
    setLogText(await api.runlogTail(200));
    setLogOpen(true);
  };

  if (loading && !overview) return <Spin />;
  if (error) return <Alert type="error" message={error} />;
  if (!overview) return null;

  return (
    <div>
      <Row justify="space-between" align="middle" style={{ marginBottom: 12 }}>
        <Typography.Title level={4} style={{ margin: 0 }}>纸面盘驾驶舱</Typography.Title>
        <ManualRunButton onStarted={() => void load()} />
      </Row>
      <Row gutter={12} style={{ marginBottom: 12 }}>
        {overview.cards.map((c) => (
          <Col key={c.book} span={8}><BookCard card={c} /></Col>
        ))}
      </Row>
      <Row gutter={12}>
        <Col span={14}><DiffTable rows={overview.diff} t={overview.diff_t} /></Col>
        <Col span={10}>
          <RunStatusPanel runlog={overview.runlog} schtask={overview.schtask} onOpenLog={() => void openLog()} />
        </Col>
      </Row>
      <Drawer title="run.log(末 200 行)" open={logOpen} onClose={() => setLogOpen(false)} width={720}>
        <pre style={{ fontSize: 12, whiteSpace: "pre-wrap" }}>{logText}</pre>
      </Drawer>
    </div>
  );
}
```

`App.tsx`：驾驶舱路由由占位改真页——

```tsx
import Cockpit from "./pages/Cockpit";
// Routes 内替换:
<Route path="/cockpit" element={<Cockpit />} />
```

（同时 `main.tsx` 用 antd `<App>` 包根组件以启用 message/modal 上下文：`import { App as AntApp } from "antd"` → `<AntApp><App /></AntApp>`。）

- [ ] **Step 5: 写测试 Cockpit.test.tsx**（mock store 的 api 字段；不碰真 invoke）

```tsx
import { render, screen, waitFor } from "@testing-library/react";
import "@testing-library/jest-dom";
import { HashRouter } from "react-router-dom";
import { App as AntApp } from "antd";
import Cockpit from "./Cockpit";
import { useCockpit } from "../stores/cockpit";
import type { OverviewDto } from "@bindings/OverviewDto";

const OVERVIEW: OverviewDto = {
  cards: [
    { book: "b1", title: "账本1 · sh600030 60m", kind: "single", status: "ok", advice: null,
      nav: 1.0539, total_return: 0.0539, max_drawdown: 0.0213, pos: 0, state_time: "2026-06-12T15:00:00",
      holdings: null, last_signal: null },
    { book: "b2", title: "账本2 · sh600036 60m", kind: "single", status: "empty",
      advice: "state 未建账:等待 15:35 schtask 首跑,或手动触发 run(收盘后)", nav: null, total_return: null,
      max_drawdown: null, pos: null, state_time: null, holdings: null, last_signal: null },
    { book: "b3", title: "账本3 · 组合 top3 日线", kind: "portfolio", status: "ok", advice: null,
      nav: null, total_return: null, max_drawdown: null, pos: null, state_time: "2026-06-11T15:00:00",
      holdings: [["sh600900", 0.5], ["sz000333", 0.5]], last_signal: null },
  ],
  diff: [{ symbol: "sh600900", action: "Hold", from_w: 0.5, to_w: 0.5 }],
  diff_t: "2026-06-12T15:00:00",
  runlog: { last_header: "==== Fri 06/12/2026 ====", ok: true, summary: "最近一次 run 正常收尾" },
  schtask: { next_run: "6/12/2026 3:35:00 PM", last_run: null, last_result: "267011", status: "Ready" },
};

test("cockpit renders three book cards, diff and run status", async () => {
  useCockpit.setState({
    api: { ...useCockpit.getState().api, cockpitOverview: async () => OVERVIEW },
  });
  render(
    <AntApp><HashRouter><Cockpit /></HashRouter></AntApp>
  );
  await waitFor(() => expect(screen.getByText("账本1 · sh600030 60m")).toBeInTheDocument());
  expect(screen.getByText(/未建账/)).toBeInTheDocument();
  expect(screen.getByText(/sh600900 0.50/)).toBeInTheDocument();
  expect(screen.getByText("最近一次 run 正常收尾")).toBeInTheDocument();
});
```

- [ ] **Step 6: 验证**

Run: `cd desktop/ui && npx tsc --noEmit && npx vitest run`
Expected: 全绿（3 个测试文件）。

- [ ] **Step 7: Commit**

```bash
git add desktop/ui/src/api/ipc.ts desktop/ui/src/stores/cockpit.ts desktop/ui/src/pages/Cockpit.tsx desktop/ui/src/pages/Cockpit.test.tsx desktop/ui/src/components/BookCard.tsx desktop/ui/src/components/DiffTable.tsx desktop/ui/src/components/RunStatusPanel.tsx desktop/ui/src/components/ManualRunButton.tsx desktop/ui/src/App.tsx desktop/ui/src/main.tsx
git commit -m "feat(desktop): cockpit overview page - cards/diff/run-status/manual-run"
```

---

### Task 13: UI 账本详情页（净值曲线 + 快照 + journal 时间线）

**Files:**
- Create: `desktop/ui/src/pages/BookDetail.tsx`、`desktop/ui/src/components/NavChart.tsx`、`desktop/ui/src/pages/BookDetail.test.tsx`
- Modify: `desktop/ui/src/App.tsx`

- [ ] **Step 1: NavChart.tsx（ECharts 折线；b3 显示成员数）**

```tsx
import { useEffect, useRef } from "react";
import * as echarts from "echarts";
import type { JournalPointDto } from "@bindings/JournalPointDto";

export default function NavChart({ points, portfolio }: { points: JournalPointDto[]; portfolio: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!ref.current) return;
    const chart = echarts.init(ref.current);
    chart.setOption({
      tooltip: { trigger: "axis" },
      xAxis: { type: "category", data: points.map((p) => p.state_time) },
      yAxis: { type: "value", scale: true },
      series: [
        portfolio
          ? { name: "成员数", type: "line", step: "end", data: points.map((p) => p.members ?? null) }
          : { name: "nav", type: "line", data: points.map((p) => p.nav ?? null) },
      ],
      grid: { left: 48, right: 16, top: 24, bottom: 24 },
    });
    const onResize = () => chart.resize();
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("resize", onResize);
      chart.dispose();
    };
  }, [points, portfolio]);
  return <div ref={ref} style={{ height: 260 }} />;
}
```

- [ ] **Step 2: BookDetail.tsx**

```tsx
import { useEffect, useState } from "react";
import { useParams, Link } from "react-router-dom";
import { Alert, Card, Descriptions, Spin, Typography } from "antd";
import type { BookDetailDto } from "@bindings/BookDetailDto";
import { api } from "../api/ipc";
import NavChart from "../components/NavChart";

export default function BookDetail() {
  const { book = "" } = useParams();
  const [data, setData] = useState<BookDetailDto | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.bookDetail(book).then(setData).catch((e) => setError(String(e)));
  }, [book]);

  if (error) return <Alert type="error" message={error} />;
  if (!data) return <Spin />;
  const s = data.snapshot;

  return (
    <div>
      <Typography.Title level={4}>
        <Link to="/cockpit">驾驶舱</Link> / {data.card.title}
      </Typography.Title>
      <Card size="small" title={data.card.kind === "portfolio" ? "持仓成员数(journal)" : "纸面净值(journal,自桌面端启用日积累)"} style={{ marginBottom: 12 }}>
        {data.journal.length ? (
          <NavChart points={data.journal} portfolio={data.card.kind === "portfolio"} />
        ) : (
          <Typography.Text type="secondary">journal 暂无数据——历史从桌面端启用日开始积累</Typography.Text>
        )}
      </Card>
      {s && (
        <Card size="small" title="AccountSnapshot(13 字段,只读)">
          <Descriptions size="small" column={3} bordered>
            <Descriptions.Item label="pos">{s.pos}</Descriptions.Item>
            <Descriptions.Item label="entry_price">{s.entry_price ?? "—"}</Descriptions.Item>
            <Descriptions.Item label="bars_held">{s.bars_held}</Descriptions.Item>
            <Descriptions.Item label="nav">{s.nav.toFixed(6)}</Descriptions.Item>
            <Descriptions.Item label="peak_nav">{s.peak_nav.toFixed(6)}</Descriptions.Item>
            <Descriptions.Item label="max_drawdown">{(s.max_drawdown * 100).toFixed(2)}%</Descriptions.Item>
            <Descriptions.Item label="turnover">{s.turnover.toFixed(4)}</Descriptions.Item>
            <Descriptions.Item label="last_increase_date">{s.last_increase_date ?? "—"}</Descriptions.Item>
            <Descriptions.Item label="max_price_since_entry">{s.max_price_since_entry ?? "—"}</Descriptions.Item>
            <Descriptions.Item label="min_price_since_entry">{s.min_price_since_entry ?? "—"}</Descriptions.Item>
            <Descriptions.Item label="bars_since_exit">{s.bars_since_exit ?? "—"}</Descriptions.Item>
            <Descriptions.Item label="last_trip_return">{s.last_trip_return ?? "—"}</Descriptions.Item>
            <Descriptions.Item label="trip">{s.trip ? JSON.stringify(s.trip) : "—"}</Descriptions.Item>
          </Descriptions>
        </Card>
      )}
    </div>
  );
}
```

`App.tsx` Routes 增加：

```tsx
import BookDetail from "./pages/BookDetail";
// Routes 内,/cockpit 之后:
<Route path="/cockpit/:book" element={<BookDetail />} />
```

- [ ] **Step 3: 测试 BookDetail.test.tsx**（mock api 模块）

```tsx
import { render, screen, waitFor } from "@testing-library/react";
import "@testing-library/jest-dom";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { vi } from "vitest";
import type { BookDetailDto } from "@bindings/BookDetailDto";

const DETAIL: BookDetailDto = {
  card: { book: "b1", title: "账本1 · sh600030 60m", kind: "single", status: "ok", advice: null,
    nav: 1.05, total_return: 0.05, max_drawdown: 0.02, pos: 1, state_time: "2026-06-12T15:00:00",
    holdings: null, last_signal: null },
  snapshot: { pos: 1, entry_price: 6.1, bars_held: 4, nav: 1.05, peak_nav: 1.06, max_drawdown: 0.02,
    turnover: 2.4, last_increase_date: "2026-06-09", max_price_since_entry: 6.3, min_price_since_entry: 6.0,
    bars_since_exit: null, last_trip_return: null, trip: null },
  journal: [{ state_time: "2026-06-12T15:00:00", nav: 1.05, pos: 1, members: null }],
};

vi.mock("../api/ipc", () => ({ api: { bookDetail: async () => DETAIL } }));
// echarts 在 jsdom 无布局,mock 掉渲染细节
vi.mock("echarts", () => ({
  init: () => ({ setOption: () => {}, resize: () => {}, dispose: () => {} }),
}));

import BookDetail from "./BookDetail";

test("book detail renders snapshot fields", async () => {
  render(
    <MemoryRouter initialEntries={["/cockpit/b1"]}>
      <Routes><Route path="/cockpit/:book" element={<BookDetail />} /></Routes>
    </MemoryRouter>
  );
  await waitFor(() => expect(screen.getByText(/账本1/)).toBeInTheDocument());
  expect(screen.getByText("bars_held")).toBeInTheDocument();
  expect(screen.getByText("1.050000")).toBeInTheDocument();
});
```

- [ ] **Step 4: 验证**

Run: `cd desktop/ui && npx tsc --noEmit && npx vitest run`
Expected: 全绿。

- [ ] **Step 5: Commit**

```bash
git add desktop/ui/src/pages/BookDetail.tsx desktop/ui/src/pages/BookDetail.test.tsx desktop/ui/src/components/NavChart.tsx desktop/ui/src/App.tsx
git commit -m "feat(desktop): book detail page - nav chart + 13-field snapshot + journal"
```

---

### Task 14: 任务抽屉 + 全量收尾闸

**Files:**
- Create: `desktop/ui/src/components/TaskDrawer.tsx`
- Modify: `desktop/ui/src/App.tsx`

- [ ] **Step 1: TaskDrawer.tsx**（事件订阅 + 兜底轮询；进行中任务可取消）

```tsx
import { useEffect, useState } from "react";
import { Badge, Button, Drawer, List, Progress, Typography } from "antd";
import { listen } from "@tauri-apps/api/event";
import type { TaskInfoDto } from "@bindings/TaskInfoDto";
import { api } from "../api/ipc";

const STATUS_BADGE: Record<string, string> = {
  running: "processing", done: "success", failed: "error", cancelled: "default",
};

export default function TaskDrawer() {
  const [open, setOpen] = useState(false);
  const [tasks, setTasks] = useState<TaskInfoDto[]>([]);

  const refresh = () => void api.taskList().then(setTasks).catch(() => {});

  useEffect(() => {
    refresh();
    // 任意任务事件都触发全量刷新(M1 任务数极少,简单正确优先)
    const un = listen("task://progress", refresh);
    const timer = setInterval(refresh, 2000);
    return () => {
      void un.then((f) => f());
      clearInterval(timer);
    };
  }, []);

  const running = tasks.filter((t) => t.status === "running").length;

  return (
    <>
      <Badge count={running} size="small">
        <Button size="small" onClick={() => setOpen(true)}>任务</Button>
      </Badge>
      <Drawer title="任务" open={open} onClose={() => setOpen(false)} width={420}>
        <List
          dataSource={tasks}
          locale={{ emptyText: "暂无任务" }}
          renderItem={(t) => (
            <List.Item
              actions={t.status === "running" ? [<a key="c" onClick={() => void api.taskCancel(t.id)}>取消</a>] : []}
            >
              <List.Item.Meta
                title={<Badge status={(STATUS_BADGE[t.status] ?? "default") as never} text={`${t.kind} · ${t.id}`} />}
                description={
                  <>
                    <Progress percent={Math.round(t.progress.pct * 100)} size="small" />
                    <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                      {t.progress.stage} {t.progress.detail} {t.error ?? ""}
                    </Typography.Text>
                  </>
                }
              />
            </List.Item>
          )}
        />
      </Drawer>
    </>
  );
}
```

> 桥接层事件名是 `task://progress/{id}`——前端用前缀监听不可行时（tauri 事件是精确匹配），改桥接层 `TauriSink::emit` 同时发一条固定事件 `task://progress`（同 payload）即可，两行改动。

`App.tsx`：侧边栏底部或 Content 顶部放 `<TaskDrawer />`（Layout.Content 第一行加一个右对齐 Row）。

- [ ] **Step 2: 全量收尾闸（M1 完成定义）**

Run（全部必须绿）：

```bash
cargo test                                              # 引擎全量
cargo test -p rquant-desktop                            # 桥接层全部
cargo clippy --workspace --all-targets -- -D warnings   # lint
cd desktop/ui && npx tsc --noEmit && npx vitest run && npm run build
```

- [ ] **Step 3: 人工冒烟清单**（开发者执行，逐项打勾记录在提交信息）

```bash
cd desktop/ui && npx tauri dev
```

- [ ] 窗口启动落驾驶舱;三卡显示真实 paper/ 数据（15:35 后 b1/b2 应为"正常"+nav）
- [ ] 点击 b1 卡 → 详情页快照 13 字段 + journal 曲线（首次打开后 journal 应有 1 点）
- [ ] "查看 run.log"抽屉显示末 200 行
- [ ] 盘后手动触发 run（不勾 commit）→ 任务抽屉出现进度 → 完成后卡片刷新,幂等(`bars_replayed=0`)
- [ ] 盘中（若在交易时段验证）commit 复选框被禁用并显示 forming bar 提示

- [ ] **Step 4: Commit**

```bash
git add desktop/ui/src/components/TaskDrawer.tsx desktop/ui/src/App.tsx
git commit -m "feat(desktop): task drawer with progress events; M1 smoke checklist passed"
```

- [ ] **Step 5: 合并回 master**

REQUIRED SUB-SKILL: `superpowers:finishing-a-development-branch`——验证全量测试 → 询问合并 → 合并 master → 删除 desktop-m1 分支。合并前贴近时点 `git log origin/master..master` 检查并行提交。

---

## 计划自审记录

- **Spec 覆盖（M1 范围）**：骨架(T1-T3,T6) / 驾驶舱 §5.1 全项（卡片 T7,T12;diff T7,T12;journal T8,T13;run.log T9,T12;schtask T9,T12;手动 run+三纪律 T10,T11,T12;快照 13 字段 T7,T13）/ 任务系统 §6（T6,T14）/ §7 原子写与互斥（T8 rename;T6 heavy 槽 + 注释说明 M1 等价性）/ §9 安全（manual_run 不触 LLM key,build_llm 空参即未配置）/ 引擎改动仅 §4-2 可见性（T10,T7 备注）。M2+ 内容（回测/留档 runs/、settings 页、工作区可改）不在本计划。
- **占位符扫描**：无 TBD/TODO;所有代码块完整可落盘。
- **类型一致性**：DTO 字段以 T5 为唯一定义,T7/T11/T12/T13 引用处逐字段核对过;`TaskRegistry::start(kind, heavy, body)` 签名 T6 定义、T11 调用一致;`GateDto.gate` 三值 "allow|dry_only|warn" 在 T10/T11/T12 一致;事件名差异已在 T14 备注给出两行修正方案。
- **已知偏差声明**：测试借用真实 `deploy/*.yaml`（拷贝到 tempdir）——树文件改名会让 fixture 测试失败,这是**故意的**（账本与冻结树的 meta.name 纪律应当由测试看护）。



