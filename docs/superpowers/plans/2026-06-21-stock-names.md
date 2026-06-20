# 股票中文名全局展示 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 客户端所有显示股票代码的地方并排显示中文名称。

**Architecture:** 方案 B——一次性抓 `data/baostock/names.csv`(symbol→名称),桥层 `names_map` 命令返回映射,前端 `useNames` store 启动加载一次 + 共享 `<SymbolLabel>` 组件渲染「名称 代码」,替换各处 symbol 渲染。零 DTO 改动。

**Tech Stack:** baostock(query_stock_basic)+ Rust(std::fs/HashMap)+ React/Zustand/antd + Vitest。复用 `paths::Workspace`、App 启动 init 范式(同 tasks/audit store)。

## Global Constraints

- 缺名(names.csv 无该 symbol / 文件缺失)→ **只显代码,绝不臆造**。
- `<SymbolLabel>` 是唯一渲染入口;`useNames` 选择器 `s.names[sym]` 返回字符串/undefined(稳定基元,**不**用返回新数组/对象的内联选择器——避 useSyncExternalStore 无限循环)。
- symbol 口径 = `sh600000`(无点,与 universe/sector_membership 一致);baostock `query_stock_basic` 的 `code`(`sh.600000`)需去点。
- names.csv 在 `.gitignore` 覆盖的 `data/` 下;`build_names.py` 提交。
- 数字/无新 DTO:`names_map` 返回 `HashMap<String,String>`(serde→JS 对象),不新增 ts-rs 类型。
- 验证三件套:`cargo test --workspace`;`node desktop/ui/node_modules/typescript/bin/tsc --noEmit -p desktop/ui/tsconfig.app.json`;`npm --prefix desktop/ui run test -- --run` + build。英文 commit(`git commit -F -`);只 add 本任务文件;不 push。

---

### Task 1: build_names.py + 生成 names.csv(联网)

**Files:** Create `scripts/build_names.py`;produces `data/baostock/names.csv`(gitignored)

- [ ] **Step 1: 实现** `scripts/build_names.py`:

```python
#!/usr/bin/env python3
"""全市场股票 symbol→中文名 → data/baostock/names.csv(symbol,name)。
baostock query_stock_basic 一次返回全部;code(sh.600000)去点归一为 sh600000;仅 type==1(股票)。"""
import os, sys, csv
import baostock as bs

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(REPO, "data", "baostock", "names.csv")

def main():
    lg = bs.login()
    if lg.error_code != "0":
        print(f"baostock login failed: {lg.error_msg}", file=sys.stderr); sys.exit(1)
    try:
        rs = bs.query_stock_basic()
        if rs.error_code != "0":
            print(f"query_stock_basic failed: {rs.error_msg}", file=sys.stderr); sys.exit(1)
        rows = []
        while rs.next():
            code, code_name, _ipo, _out, typ, _status = (rs.get_row_data() + ["", "", "", "", "", ""])[:6]
            if typ != "1":  # 1=股票
                continue
            sym = code.replace(".", "")
            if sym and code_name:
                rows.append((sym, code_name))
    finally:
        bs.logout()
    if not rows:
        print("no stock rows returned — refusing to write empty names.csv", file=sys.stderr); sys.exit(1)
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "w", newline="", encoding="utf-8") as f:
        w = csv.writer(f); w.writerow(["symbol", "name"]); w.writerows(rows)
    print(f"wrote {OUT}: {len(rows)} names")

if __name__ == "__main__":
    main()
```

- [ ] **Step 2: 跑(联网)** `python scripts/build_names.py` → "wrote ...names.csv: N names"(N 数千)。
- [ ] **Step 3: 核对** `head -3 data/baostock/names.csv`(表头 + 形如 `sh600000,浦发银行`);`grep -E "^sh600016," data/baostock/names.csv`(应 `sh600016,民生银行`)。
- [ ] **Step 4: Commit**(仅脚本;names.csv 被 gitignore)

```bash
git add scripts/build_names.py
git commit -F - <<'EOF'
feat(data): build_names.py — baostock symbol→Chinese-name → names.csv
EOF
```

---

### Task 2: 桥层 names 加载器 + 命令(TDD)

**Files:** Create `desktop/src-tauri/src/names.rs`;Modify `paths.rs`、`lib.rs`

**Interfaces — Produces:** `names::load_names(path:&Path) -> std::collections::HashMap<String,String>`;`paths::Workspace::names_path()`;命令 `names_map(state) -> HashMap<String,String>`。

- [ ] **Step 1: paths.rs** 加 `pub fn names_path(&self) -> PathBuf { self.root().join("data").join("baostock").join("names.csv") }`(近 kday_dir)。

- [ ] **Step 2: 写失败测试** `names.rs`(`#[cfg(test)]`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loads_symbol_name_map_skips_header_and_bad() {
        let td = tempfile::tempdir().unwrap();
        let p = td.path().join("names.csv");
        std::fs::write(&p, "symbol,name\nsh600016,民生银行\nsz000001,平安银行\nbadline\n,emptySym\nsh600000,\n").unwrap();
        let m = load_names(&p);
        assert_eq!(m.get("sh600016").map(String::as_str), Some("民生银行"));
        assert_eq!(m.get("sz000001").map(String::as_str), Some("平安银行"));
        assert_eq!(m.len(), 2); // badline / 空 symbol / 空 name 均跳过
    }
    #[test]
    fn missing_file_is_empty() {
        assert!(load_names(std::path::Path::new("E:/nonexistent/names.csv")).is_empty());
    }
}
```

- [ ] **Step 3: 跑确认失败** `cargo test -p rquant-desktop names:: 2>&1 | tail -6` → FAIL。

- [ ] **Step 4: 实现** `names.rs` 顶部:

```rust
//! 股票 symbol→中文名 映射(读 data/baostock/names.csv;缺文件/缺名容错)。
use std::collections::HashMap;
use std::path::Path;

pub fn load_names(path: &Path) -> HashMap<String, String> {
    let mut m = HashMap::new();
    if let Ok(txt) = std::fs::read_to_string(path) {
        for line in txt.lines().skip(1) {
            if let Some((sym, name)) = line.split_once(',') {
                let (sym, name) = (sym.trim(), name.trim());
                if !sym.is_empty() && !name.is_empty() {
                    m.insert(sym.to_string(), name.to_string());
                }
            }
        }
    }
    m
}
```

- [ ] **Step 5: 命令** `commands.rs`(或新 `names.rs` 内加命令;放 commands.rs 薄壳处更一致):

```rust
#[tauri::command]
pub fn names_map(state: tauri::State<AppState>) -> std::collections::HashMap<String, String> {
    crate::names::load_names(&state.ws.names_path())
}
```

`lib.rs`:加 `pub mod names;` + `generate_handler!` 注册 `commands::names_map`(若命令放 commands.rs)。

- [ ] **Step 6: 跑确认通过** `cargo test -p rquant-desktop names:: 2>&1 | tail -6` → PASS;`cargo test -p rquant-desktop 2>&1 | grep "test result"` 全绿;`cargo build -p rquant-desktop` 绿。

- [ ] **Step 7: Commit**

```bash
git add desktop/src-tauri/src/names.rs desktop/src-tauri/src/paths.rs desktop/src-tauri/src/commands.rs desktop/src-tauri/src/lib.rs
git commit -F - <<'EOF'
feat(desktop): names loader + names_map command (symbol→Chinese name)
EOF
```

---

### Task 3: 前端 names store + SymbolLabel + App 启动加载(TDD)

**Files:** Create `desktop/ui/src/stores/names.ts`、`stores/names.test.ts`、`components/SymbolLabel.tsx`、`components/SymbolLabel.test.tsx`;Modify `api/ipc.ts`、`App.tsx`

**Interfaces — Produces:** `useNames`(zustand: `{api, names: Record<string,string>, loaded, load()}`);`<SymbolLabel symbol={s}/>`。

- [ ] **Step 1: ipc.ts 追加** `namesMap: () => invoke<Record<string, string>>("names_map"),`。

- [ ] **Step 2: store + 失败测试**:

`stores/names.ts`:
```typescript
import { create } from "zustand";
import { api as realApi, type Api } from "../api/ipc";
interface NamesState { api: Api; names: Record<string, string>; loaded: boolean; load: () => Promise<void>; }
export const useNames = create<NamesState>((set, get) => ({
  api: realApi, names: {}, loaded: false,
  load: async () => { try { set({ names: await get().api.namesMap(), loaded: true }); } catch { /* 缺名静默,只显代码 */ } },
}));
```
`stores/names.test.ts`:
```typescript
import { test, expect, afterEach } from "vitest";
import { useNames } from "./names";
const real = useNames.getState().api;
afterEach(() => useNames.setState({ api: real, names: {}, loaded: false }));
test("load fills names from api", async () => {
  useNames.setState({ api: { ...real, namesMap: async () => ({ sh600016: "民生银行" }) } });
  await useNames.getState().load();
  expect(useNames.getState().names["sh600016"]).toBe("民生银行");
  expect(useNames.getState().loaded).toBe(true);
});
```

- [ ] **Step 3: 跑失败** `npm --prefix desktop/ui run test -- --run src/stores/names.test.ts 2>&1 | tail -6` → FAIL。

- [ ] **Step 4: SymbolLabel + 测试**:

`components/SymbolLabel.tsx`:
```tsx
import { useNames } from "../stores/names";
/** 渲染「名称 代码(灰)」;无名则只显代码。代码口径 sh600000。 */
export default function SymbolLabel({ symbol }: { symbol: string }) {
  const name = useNames((s) => s.names[symbol]);
  if (!name) return <span>{symbol}</span>;
  return <span>{name} <span style={{ color: "#999", fontSize: 12 }}>{symbol}</span></span>;
}
```
`components/SymbolLabel.test.tsx`:
```tsx
import { test, expect, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { useNames } from "../stores/names";
import SymbolLabel from "./SymbolLabel";
afterEach(() => useNames.setState({ names: {} }));
test("shows name + code when known", () => {
  useNames.setState({ names: { sh600016: "民生银行" } });
  render(<SymbolLabel symbol="sh600016" />);
  expect(screen.getByText(/民生银行/)).toBeTruthy();
  expect(screen.getByText("sh600016")).toBeTruthy();
});
test("shows only code when unknown", () => {
  render(<SymbolLabel symbol="sh999999" />);
  expect(screen.getByText("sh999999")).toBeTruthy();
});
```

- [ ] **Step 5: App.tsx** — 在 `Shell()` 的 init effect 里加载(同 `useTasks.init()` 处):`import { useNames } from "./stores/names";` + 在现有 `useEffect(() => { useTasks.getState().init(); ... }, [])` 内补 `useNames.getState().load();`(或新 effect)。

- [ ] **Step 6: 跑通过** `npm --prefix desktop/ui run test -- --run src/stores/names.test.ts src/components/SymbolLabel.test.tsx 2>&1 | tail -8` PASS;`tsc --noEmit` 0;`npm ... test --run` 全绿。

- [ ] **Step 7: Commit**

```bash
git add desktop/ui/src/stores/names.ts desktop/ui/src/stores/names.test.ts desktop/ui/src/components/SymbolLabel.tsx desktop/ui/src/components/SymbolLabel.test.tsx desktop/ui/src/api/ipc.ts desktop/ui/src/App.tsx
git commit -F - <<'EOF'
feat(ui): names store + SymbolLabel component + load on app start
EOF
```

---

### Task 4: 各处 symbol 渲染替换为 SymbolLabel

**Files:** Modify(READ 各文件确认 symbol 渲染处再替换)`components/ScreenPickTable.tsx`、`components/DiffTable.tsx`、`pages/Deploy.tsx`、`pages/Cockpit.tsx`、`components/RunTradesView`(或回测交易表所在,READ `pages/Backtest.tsx` 找)、`components/ReplayView.tsx`、`components/ScreenBacktestResult.tsx`、`pages/BookDetail.tsx`

**Interfaces — Consumes:** `<SymbolLabel>`(Task 3)。

- [ ] **Step 1: 全量定位** `grep -rnE "\.symbol|symbol[}: ]" desktop/ui/src/{components,pages} | grep -iv import` 找出所有把 `symbol` 当文本渲染的 JSX 处(表格 column render、cell、行内文本)。逐个确认是"展示给用户的股票代码"(非内部 key/参数)。

- [ ] **Step 2: 替换** 每处 `{x.symbol}` / `render: (s) => s`(symbol 列)→ `<SymbolLabel symbol={x.symbol} />`。`import SymbolLabel from "../components/SymbolLabel";`。表格 symbol 列可保留列标题「标的」,单元格渲染 `<SymbolLabel>`;行内/标签(如 DiffTable 的买卖行、Deploy 持仓、回测交易 symbol、回放 symbol)同。**不动** `rowKey={...symbol}`、传参、配置路径里的 symbol(只换面向用户的展示文本)。

- [ ] **Step 3: 验证** `node desktop/ui/node_modules/typescript/bin/tsc --noEmit -p desktop/ui/tsconfig.app.json 2>&1 | tail -8` → 0;`npm --prefix desktop/ui run test -- --run 2>&1 | tail -6` 全绿(既有表格测试若断言纯 symbol 文本,可能需放宽为包含 symbol——READ 调整,保持断言有效);`grep -rn "SymbolLabel" desktop/ui/src/{components,pages} | grep -v "SymbolLabel.tsx" | wc -l` 列出接入点数。

- [ ] **Step 4: Commit**

```bash
git add desktop/ui/src/components desktop/ui/src/pages
git commit -F - <<'EOF'
feat(ui): render stock Chinese name (SymbolLabel) wherever symbols appear
EOF
```

---

### Task 5: 收尾闸 + 文档 + 记忆

- [ ] **Step 1: 全量闸** `cargo test --workspace 2>&1 | grep "test result"` 全 ok;`tsc --noEmit` 0;`npm --prefix desktop/ui run test -- --run` 全过;`npm --prefix desktop/ui run build` 成功。
- [ ] **Step 2: 数据核验** `test -f data/baostock/names.csv && wc -l data/baostock/names.csv`(数千行);`grep -c , data/baostock/names.csv`。
- [ ] **Step 3: GUI 冒烟**(release `cargo tauri dev --release --no-watch`):选股榜跑一次 → 标的列显示「名称 代码」;部署持仓/调仓清单、驾驶舱组合 diff、回测交易表同;构造一个不在 names.csv 的代码 → 仅显代码(不崩)。
- [ ] **Step 4: 文档 + 记忆** `docs/desktop-screen-research.md` 加「股票中文名」一句节;更新记忆 `rquant-project.md`(stock-names 落地 + names.csv 来源/刷新 + SymbolLabel 全局入口);`baostock-fetch-scheduled-task.md` 或数据节补 `build_names.py` 一行(名称数据来源)。
- [ ] **Step 5: Commit + finishing**

```bash
git add docs/ scripts/ && git commit -F - <<'EOF'
docs(desktop): stock-names usage; finalize
EOF
```
调用 superpowers:finishing-a-development-branch 收口。

---

## 自审备忘(写计划时已校)

- **spec 覆盖**:数据(build_names.py/names.csv)→T1;桥层 load_names+names_map+paths→T2;前端 store+SymbolLabel+App init→T3;所有 symbol 渲染点替换→T4;闸/文档/记忆/finishing→T5。
- **类型一致**:`load_names`/`names_path`/`names_map`(后端)↔`namesMap`/`useNames`/`SymbolLabel`(前端)贯穿;命令返回 `HashMap<String,String>`↔ipc `Record<string,string>`。
- **诚实/YAGNI**:缺名只显代码(T2 容错 + SymbolLabel 兜底 + store load catch);无新 DTO;无实时/搜名;names 周期抓。
- **footgun 防护**:`useNames(s=>s.names[symbol])` 返回基元,不触发 useSyncExternalStore 循环。
- **已知依赖**:T1 联网(baostock query_stock_basic);T4 各渲染点需 READ 真实变量名/列定义对齐(只换面向用户展示,不动 rowKey/参数)。
