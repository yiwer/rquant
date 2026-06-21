# 15m 选股并行模块 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans. Steps use checkbox (`- [ ]`).

**Goal:** 选股页新增「15m选股（实验）」tab，与日线选股并行；复用 `run_screen` 跑 15m universe(k15m bar + features_15m 因子)，占位可配置框架。

**Architecture:** 引擎零改动。15m = `run_screen(universe_baostock_15m_feat.csv, 15m 配置)`；桥层加 2 命令镜像日线；前端加 1 tab + store 平行态。详见 [spec](../specs/2026-06-21-intraday-15m-screen-module-design.md)。

**Tech Stack:** Rust(screen_cmds 薄壳 + TaskRegistry)+ React/Zustand/antd + Python(universe 生成) + Vitest。复用 `ScreenResultDto`/`ScreenPickTable`/全局任务 store/`SymbolLabel`/`friendlyError`。

## Global Constraints

- **诚实**：模块全程标注「实验·因子无验证 edge·sina 幸存者偏差·无 OOS」（UI 红字 banner + 配置注释 + 文档）。不预置那 12 棵证伪树；占位树明确"仅示例，自行替换"。
- **零引擎改动**：不动 `src/`；不动日线选股/部署。新增都在新文件 + screen_cmds 加 2 命令 + Screen.tsx 加 1 tab + store/ipc 平行态。
- **数据口径**：15m universe 的 `fundamentals` 列指向 `features_15m/<sym>.csv`（31 个 15m 指标，date-keyed），树经 `fund.<col>` 取用。`primary`=`k15m/<sym>.csv`。绝对路径 + 正斜杠（同 `universe_baostock_day.csv`）。data/ 已 gitignore（universe CSV 不提交，脚本提交）。
- **验证三件套**：`cargo test --workspace`（桥层 API 动了 → 必须 --workspace）；`node desktop/ui/node_modules/typescript/bin/tsc --noEmit -p desktop/ui/tsconfig.app.json`；`npm --prefix desktop/ui run test -- --run` + build。英文 commit（`git commit -F -`）；只 add 本任务文件；不 push。

---

### Task 1: 15m universe 生成脚本 + 生成

**Files:** Create `scripts/build_universe_15m_feat.py`；produces `data/baostock/universe_baostock_15m_feat.csv`（gitignored）

- [ ] **Step 1: 写脚本** `scripts/build_universe_15m_feat.py`:

```python
#!/usr/bin/env python3
"""生成 15m 选股 universe：symbol→primary=k15m, fundamentals=features_15m(31 个 15m 指标)。
取同时有 k15m/<sym>.csv 且 features_15m/<sym>.csv 的 symbol；绝对路径+正斜杠(同 universe_baostock_day.csv)。"""
import os, csv, glob
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
K15M = os.path.join(REPO, "data", "baostock", "k15m")
FEAT = os.path.join(REPO, "data", "baostock", "features_15m")
OUT  = os.path.join(REPO, "data", "baostock", "universe_baostock_15m_feat.csv")

def main():
    syms = sorted(
        os.path.basename(p)[:-4] for p in glob.glob(os.path.join(K15M, "*.csv"))
        if os.path.exists(os.path.join(FEAT, os.path.basename(p)))
    )
    if not syms:
        raise SystemExit("no symbols with both k15m and features_15m")
    with open(OUT, "w", newline="", encoding="utf-8") as f:
        w = csv.writer(f); w.writerow(["symbol", "primary", "context", "fundamentals"])
        for s in syms:
            w.writerow([s, os.path.join(K15M, f"{s}.csv").replace("\\", "/"),
                        "", os.path.join(FEAT, f"{s}.csv").replace("\\", "/")])
    print(f"wrote {OUT}: {len(syms)} symbols")

if __name__ == "__main__":
    main()
```

- [ ] **Step 2: 跑** `python scripts/build_universe_15m_feat.py` → "wrote ...: ~1034 symbols"。
- [ ] **Step 3: 核对** `head -2 data/baostock/universe_baostock_15m_feat.csv`（表头 `symbol,primary,context,fundamentals` + 形如 `sh600000,E:/.../k15m/sh600000.csv,,E:/.../features_15m/sh600000.csv`）；`wc -l`（~1035 行含表头）。
- [ ] **Step 4: Commit**（仅脚本；universe CSV gitignored）

```bash
git add scripts/build_universe_15m_feat.py
git commit -F - <<'EOF'
feat(data): build_universe_15m_feat.py — 15m screen universe (k15m bars + features_15m factors)
EOF
```

---

### Task 2: 占位 15m 配置 + 示例树 + CLI 冒烟

**Files:** Create `examples/trees/screen/intraday15m_example.yaml`、`examples/screen/intraday/15m_placeholder.yaml`

**Interfaces — Produces:** 配置 `examples/screen/intraday/15m_placeholder.yaml`（Task 3 桥层默认列举此目录）。

- [ ] **Step 1: 示例占位树** `examples/trees/screen/intraday15m_example.yaml`:

```yaml
# ⚠️ 示例占位树（仅演示 15m 因子接法，无验证 edge，请自行替换）。
# 15m 因子经 fund.<col> 取 features_15m 列：ret/amplitude/ma5..ma60/ema12/ema26/volma5/volma20/
#   macd_dif|macd_dea|macd_hist/rsi14/boll_mid|boll_up|boll_dn|boll_pctb|boll_bw/atr14/
#   kdj_k|kdj_d|kdj_j/cci14/wr14/obv/vwap20/roc12/rvol20/corr_pv20。
# 本例：按相对成交量 rvol20 排序(放量高分)。gate 仅排除缺数据。
meta: { name: intraday15m_example, forward_window: 1, stances: [long, flat] }
params: { rv_scale: 0.5 }
root: gate
nodes:
  gate:
    type: quant
    branches:
      - { when: "fund.rvol20 > 0", goto: g, label: ok }
    default: { goto: flat, label: flat }
leaves:
  g: { stance: long, weight: "sigmoid((fund.rvol20 - 1) / rv_scale)" }
  flat: { stance: flat }
```

- [ ] **Step 2: 占位配置** `examples/screen/intraday/15m_placeholder.yaml`:

```yaml
# 15m 选股·占位可配置框架（实验；因子无验证 edge）。universe=data/baostock/universe_baostock_15m_feat.csv。
# 因子经 fund.<col> 取 features_15m 的 31 个 15m 指标(见示例树注释)。
# 替换/新增 quality_trees 指向你的 15m 因子树即可迭代；当前仅挂一棵示例占位树(按 rvol20 放量)。
quality_trees: [examples/trees/screen/intraday15m_example.yaml]
setup_trees:
  inert: [examples/trees/screen/momentum_xs.yaml]
merge: { q_floor: 0.0, top: 50, lambda: 0.0, tilt_setups: ["inert"], quality_layers: 5 }
regimes:
  - { label: "train", from: 2021-01-04, to: 2024-12-31 }
  - { label: "2025-26_OOS", from: 2025-01-01, to: 2026-06-18 }
```

- [ ] **Step 3: CLI 冒烟**（确认引擎读 k15m + features_15m via fund.<col>，as-of 选 50）：

```bash
mkdir -p .daily_runs
target/release/rquant.exe screen --config examples/screen/intraday/15m_placeholder.yaml \
  --universe data/baostock/universe_baostock_15m_feat.csv --as-of 2026-06-18 --top 50 --window 60 \
  --out .daily_runs/_15m_smoke.json >/dev/null 2>.daily_runs/_15m_smoke.err
python -c "import json;d=json.load(open('.daily_runs/_15m_smoke.json'));s=[r for r in d['rows'] if r['selected']];print('as_of',d['as_of'],'universe',d['n_universe'],'selected',len(s));print(s[:5] and [r['symbol'] for r in s[:5]])" 2>&1 || tail -5 .daily_runs/_15m_smoke.err
```
Expected: `selected 50`（universe ~1034），无报错。若报错（如 fund.rvol20 解析）→ READ features_15m 头确认列名后修树/配置再跑。

- [ ] **Step 4: Commit**

```bash
git add examples/trees/screen/intraday15m_example.yaml examples/screen/intraday/15m_placeholder.yaml
git commit -F - <<'EOF'
feat(screen): 15m placeholder config + example tree (configurable intraday factor framework)
EOF
```

---

### Task 3: 桥层 — screen_15m_asof + screen_15m_configs_list

**Files:** Modify `desktop/src-tauri/src/screen_cmds.rs`、`desktop/src-tauri/src/lib.rs`

**Interfaces — Consumes:** universe (Task 1)、配置目录 `examples/screen/intraday/` (Task 2)。**Produces:** 命令 `screen_15m_asof(config,as_of,top)->Result<String,String>`(任务id)、`screen_15m_configs_list()->Vec<ScreenConfigDto>`。

- [ ] **Step 1: screen_cmds.rs 追加**（文件顶部 use 后加常量；末尾加 2 命令）:

```rust
const SCREEN_15M_UNIVERSE: &str = "data/baostock/universe_baostock_15m_feat.csv";

#[tauri::command]
pub fn screen_15m_configs_list(state: tauri::State<AppState>) -> Vec<ScreenConfigDto> {
    let dir = state.ws.root().join("examples/screen/intraday");
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(&dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        let is_yaml = p.extension().and_then(|s| s.to_str()).map(|x| x == "yaml" || x == "yml").unwrap_or(false);
        if !is_yaml { continue }
        let rel = p.strip_prefix(state.ws.root()).unwrap_or(&p).to_string_lossy().replace('\\', "/");
        match rquant::screen::config::load_screen_config(&p) {
            Ok(_) => out.push(ScreenConfigDto { path: rel, name: p.file_stem().and_then(|s| s.to_str()).map(String::from), frozen: false, error: None }),
            Err(e) => out.push(ScreenConfigDto { path: rel, name: None, frozen: false, error: Some(format!("配置解析失败: {e}")) }),
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

#[tauri::command]
pub fn screen_15m_asof(state: tauri::State<AppState>, config: String, as_of: String, top: u32) -> Result<String, String> {
    let ws = state.ws.clone();
    state.tasks.start("screen_15m_asof", true, move |ctx| {
        if !as_of.is_empty() {
            chrono::NaiveDate::parse_from_str(&as_of, "%Y-%m-%d").map_err(|_| format!("日期格式应为 YYYY-MM-DD: {as_of}"))?;
        }
        let universe_path = ws.root().join(SCREEN_15M_UNIVERSE);
        let config_path = ws.root().join(&config);
        ctx.note_params(serde_json::json!({"config": &config, "as_of": &as_of, "top": top, "axis": "15m"}));
        ctx.note_file(&universe_path.to_string_lossy().into_owned());
        ctx.note_file(&config_path.to_string_lossy().into_owned());
        log::info!("screen_15m_asof: config={config} as_of={as_of} top={top}");
        ctx.progress(0.1, "加载", &config);
        let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
        let llm = rquant::cli::build_llm(String::new(), String::new(), ws.root().join(".rquant-cache").join("llm")).map_err(|e| e.to_string())?;
        let cfg = rquant::screen::ScreenRunConfig {
            config_path, universe_path,
            as_of: chrono::NaiveDate::parse_from_str(&as_of, "%Y-%m-%d").ok(),
            top: Some(top as usize), window: 60, out_path: None,
            membership_path: None, sectors_path: None,
        };
        ctx.progress(0.4, "选股(15m)", "");
        let res = rt.block_on(rquant::screen::run_screen(&cfg, &llm)).map_err(|e| e.to_string())?;
        ctx.note_summary(&format!("15m universe {} top {}", res.n_universe, res.top));
        let rows = res.rows.iter().map(|r| ScreenPickDto {
            rank: r.rank, symbol: r.symbol.clone(),
            quality_score: r.quality_score, speculative_score: r.speculative_score, combined_score: r.combined_score,
            tags: r.tags.clone(), selected: r.selected,
            reasons: r.reasons.iter().map(|x| ScreenReasonDto { tree: x.tree.clone(), leaf: x.leaf.clone(), score: x.score }).collect(),
        }).collect();
        let dto = ScreenResultDto { config, as_of: res.as_of.format("%Y-%m-%d").to_string(), n_universe: res.n_universe, top: res.top, rows };
        serde_json::to_value(dto).map_err(|e| e.to_string())
    })
}
```

- [ ] **Step 2: 注册** `lib.rs` 的 `tauri::generate_handler![...]` 加 `screen_cmds::screen_15m_asof, screen_cmds::screen_15m_configs_list,`（紧跟现有 `screen_cmds::screen_asof` 处；READ 确认 handler 宏位置）。

- [ ] **Step 3: 闸** `cargo test -p rquant-desktop 2>&1 | grep "test result"`（全 ok，146+ 不减）；`cargo build -p rquant-desktop 2>&1 | tail -1`（Finished）。

- [ ] **Step 4: Commit**

```bash
git add desktop/src-tauri/src/screen_cmds.rs desktop/src-tauri/src/lib.rs
git commit -F - <<'EOF'
feat(desktop): screen_15m_asof + screen_15m_configs_list bridge (parallel 15m selection)
EOF
```

---

### Task 4: 前端 — 15m tab + store 平行态 + ipc

**Files:** Modify `desktop/ui/src/api/ipc.ts`、`desktop/ui/src/stores/screen.ts`、`desktop/ui/src/pages/Screen.tsx`；Test `desktop/ui/src/stores/screen.test.ts`(若不存在则创建)

**Interfaces — Consumes:** 命令 `screen_15m_asof`/`screen_15m_configs_list` (Task 3)。

- [ ] **Step 1: ipc.ts** 加（`screenAsof` 行附近）+ 确保进 `Api` 类型（READ ipc.ts 的 `api` 对象 + `export type Api = typeof api`，若是 typeof 则自动含）:

```typescript
  screen15mConfigsList: () => invoke<import("@bindings/ScreenConfigDto").ScreenConfigDto[]>("screen_15m_configs_list"),
  screen15mAsof: (config: string, asOf: string, top: number) => invoke<string>("screen_15m_asof", { config, asOf, top }),
```

- [ ] **Step 2: stores/screen.ts** 加 15m 平行态（interface + 初值 + 2 action，镜像 configs/asof）:

interface `ScreenState` 加:
```typescript
  configs15m: ScreenConfigDto[];
  i15mTaskId: string | null;
  i15mResult: ScreenResultDto | null;
  i15mError: string | null;
  load15mConfigs: () => Promise<void>;
  run15mAsof: (config: string, asOf: string, top: number) => Promise<void>;
```
初值（`create` 默认对象内）：`configs15m: [], i15mTaskId: null, i15mResult: null, i15mError: null,`。
action（放 runAsof 后）:
```typescript
  load15mConfigs: async () => {
    try { set({ configs15m: await get().api.screen15mConfigsList() }); } catch { /* 启动早期静默 */ }
  },
  run15mAsof: async (config, asOf, top) => {
    set({ i15mTaskId: null, i15mResult: null, i15mError: null });
    try {
      const id = await get().api.screen15mAsof(config, asOf, top);
      set({ i15mTaskId: id });
      trackTask(id, {
        done: (info) => { if (get().i15mTaskId === info.id) set({ i15mResult: (info.result as ScreenResultDto | null) ?? null }); },
        failed: (info) => { if (get().i15mTaskId === info.id) set({ i15mError: friendlyError(info.error ?? "15m选股失败").title }); },
        cancelled: (info) => { if (get().i15mTaskId === info.id) set({ i15mError: "已取消" }); },
      });
    } catch (e) { set({ i15mError: friendlyError(String(e)).title }); }
  },
```

- [ ] **Step 3: Screen.tsx** 加 `Intraday15mTab`（镜像 `AsofTab`，用 15m 平行态 + 红字 banner）+ 第 3 tab:

```tsx
/** 15m 选股（实验）——镜像 AsofTab，跑 15m universe；红字标注无验证 edge。 */
function Intraday15mTab() {
  const st = useScreen();
  const [config, setConfig] = useState<string>("");
  const [asOf, setAsOf] = useState<string>("");
  const [top, setTop] = useState<number>(50);
  useEffect(() => { void st.load15mConfigs(); }, []);
  const info = useTaskInfo(st.i15mTaskId);
  const startedAt = useTaskStartedAt(st.i15mTaskId);
  const running = info?.status === "running";
  return (
    <div>
      <div style={{ color: "#dc2626", fontSize: 12, marginBottom: 8 }}>
        ⚠️ 实验模块：15m 因子无验证 edge、数据有幸存者偏差/无 OOS。占位配置仅供迭代因子，勿当已验证策略。
      </div>
      <Card size="small" style={{ marginBottom: 8 }}>
        <Row gutter={8} align="middle">
          <Col flex="auto">
            <Select style={{ width: "100%" }} placeholder="15m 选股配置" value={config || undefined}
              onChange={setConfig} options={st.configs15m.map((c) => ({ value: c.path, label: c.name ?? c.path }))} />
          </Col>
          <Col><DatePicker placeholder="指定日" onChange={(_, s) => setAsOf((s ?? "") as string)} /></Col>
          <Col><InputNumber addonBefore="数量" min={1} value={top} onChange={(v) => setTop(v ?? 50)} /></Col>
          <Col><Button type="primary" loading={running} disabled={!config || running}
            onClick={() => { if (!config || !asOf) { return; } void st.run15mAsof(config, asOf, top); }}>运行选股</Button></Col>
        </Row>
      </Card>
      {running && info ? (
        <TaskRunning info={info} startedAt={startedAt} onCancel={() => st.i15mTaskId && void st.api.taskCancel(st.i15mTaskId)} />
      ) : st.i15mError ? (
        <span style={{ color: "#dc2626" }}>{st.i15mError}</span>
      ) : st.i15mResult ? (
        <ScreenPickTable result={st.i15mResult} />
      ) : (
        <span style={{ opacity: 0.6 }}>选 15m 配置与指定日，点「运行选股」查看 15m 选股榜（尾盘截面）。</span>
      )}
    </div>
  );
}
```
`Screen()` 的 `Tabs.items` 加第 3 项：
```tsx
      { key: "intraday15m", label: "15m选股（实验）", children: <Intraday15mTab /> },
```

- [ ] **Step 4: vitest** `desktop/ui/src/stores/screen.test.ts`（不存在则建；存在则加用例）—— run15mAsof 注 mock api + 假 trackTask 终态填 i15mResult:

```typescript
import { test, expect, afterEach } from "vitest";
import { useScreen } from "./screen";
const real = useScreen.getState().api;
afterEach(() => useScreen.setState({ api: real, configs15m: [], i15mTaskId: null, i15mResult: null, i15mError: null }));
test("run15mAsof sets task id from screen15mAsof", async () => {
  useScreen.setState({ api: { ...real, screen15mAsof: async () => "t15m" } });
  await useScreen.getState().run15mAsof("examples/screen/intraday/15m_placeholder.yaml", "2026-06-18", 50);
  expect(useScreen.getState().i15mTaskId).toBe("t15m");
});
test("load15mConfigs fills configs15m", async () => {
  useScreen.setState({ api: { ...real, screen15mConfigsList: async () => [{ path: "p", name: "n", frozen: false, error: null }] } });
  await useScreen.getState().load15mConfigs();
  expect(useScreen.getState().configs15m.length).toBe(1);
});
```

- [ ] **Step 5: 闸** `node desktop/ui/node_modules/typescript/bin/tsc --noEmit -p desktop/ui/tsconfig.app.json`（0）；`npm --prefix desktop/ui run test -- --run 2>&1 | tail -6`（全绿）；`npm --prefix desktop/ui run build 2>&1 | tail -3`（成功）。

- [ ] **Step 6: Commit**

```bash
git add desktop/ui/src/api/ipc.ts desktop/ui/src/stores/screen.ts desktop/ui/src/stores/screen.test.ts desktop/ui/src/pages/Screen.tsx
git commit -F - <<'EOF'
feat(ui): 15m intraday screen tab (parallel to daily, experimental label)
EOF
```

---

### Task 5: 收尾闸 + 文档 + 记忆

- [ ] **Step 1: 全量闸** `cargo test --workspace 2>&1 | grep "test result"` 全 ok；`tsc --noEmit` 0；`npm --prefix desktop/ui run test -- --run` 全过；`npm --prefix desktop/ui run build` 成功。
- [ ] **Step 2: GUI 冒烟**（release `cargo tauri dev --release --no-watch`，CWD=desktop/src-tauri，beforeDevCommand 空 + 另起 vite）：选股页 →「15m选股（实验）」tab → 选 `15m_placeholder.yaml` + 最近日(2026-06-18) + 运行 → 排行榜非空(按 rvol20)；红字 banner 在；日线 tab 不受影响。
- [ ] **Step 3: 文档** `docs/desktop-screen-research.md` 加「15m 选股（实验）」一节（模块定位/数据/诚实边界/占位框架怎么加因子）。
- [ ] **Step 4: 记忆** 更新 `rquant-project.md`：15m 选股模块落地（GUI tab + screen_15m 命令 + universe_15m_feat + 占位框架；实验性无验证 edge；因子经 features_15m fund.<col>）。
- [ ] **Step 5: Commit + finishing**

```bash
git add docs/
git commit -F - <<'EOF'
docs(desktop): 15m intraday screen module usage; finalize
EOF
```
调用 superpowers:finishing-a-development-branch 收口。

---

## 自审备忘（写计划时已校）

- **spec 覆盖**：universe(T1)→占位配置/树(T2)→桥 2 命令(T3)→tab+store+ipc(T4)→闸/文档/记忆/finishing(T5)。
- **类型一致**：`screen_15m_asof/screen_15m_configs_list`(后端) ↔ `screen15mAsof/screen15mConfigsList`(ipc) ↔ `run15mAsof/load15mConfigs/configs15m/i15mResult`(store) ↔ `Intraday15mTab`(页面)；复用 `ScreenResultDto/ScreenPickDto/ScreenReasonDto/ScreenConfigDto`（零新 DTO）。
- **诚实/YAGNI**：仅 as-of（无 15m 回测 tab）；占位树标注无 edge；UI 红字 banner；不动日线/部署/引擎。
- **复用**：run_screen / ScreenPickTable / 全局任务 store(trackTask) / TaskRunning / SymbolLabel / friendlyError。
- **已知依赖**：T2 CLI 冒烟依赖 T1 universe；T3 注册点需 READ lib.rs handler 宏；T4 ipc `Api` 若 `typeof api` 则自动纳入新函数（READ 确认）。
