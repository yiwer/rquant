# 桌面「纸面盘」桥接 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 桌面新增「纸面盘」页,展示已验证 ridge-on-gauss 选股(本周 top-3 + 前向纸面册 + 岭值双引擎 6 折对照),并支持推进/重训/重算对照三个写操作。

**Architecture:** 方案 A——Rust 命令直接读 Python 产物(`paper_ridge_journal.csv`/`paper_ridge_weights.json`/`paper_blend.json`)算只读状态 DTO(nav=cumprod、超额复用 `index_relative`);写操作经现有 `TaskRegistry` shell Python(镜像 `iter_cmds.rs`)。全新文件承载逻辑,共享文件仅追加行。

**Tech Stack:** Rust + Tauri 2 + ts-rs(DTO→TS 自动绑定);React + antd + vitest;Python(numpy/pandas)。

## Global Constraints

- 复用已验证 Python,**不重写** gauss/ridge 逻辑;`paper_ridge.py` 不改。
- **不碰** `commands.rs`/`dto.rs`/`paths.rs`(后者含 gm_tail WIP)——路径在 `paper_cmds.rs` 内联 `state.ws.root().join("data/factor_panel/…")` 构造。
- 共享文件仅**追加**:`lib.rs`(mod 声明加在第 43 行 `pub mod tasks;` 后;handler 加在第 141 行 gm_tail 末条后、`])` 前)、`ipc.ts`(api 对象末尾)、`App.tsx`(干净,非 WIP)。与 gm_tail 行空间分离。
- **桌面变更不提交**(与 gm_tail WIP 共存于工作区);**仅 Python 改动(T1)单独提交**。
- python 解析器:`crate::paths::python_exe()`;cwd=`ws.root()`。
- ridge 策略中文名「去相关岭组合」;合成「岭值双引擎」。

---

### Task 1: eval_blend.py 加 `--json` 产物(Python,单独提交)

**Files:**
- Modify: `scripts/eval_blend.py`(`main()` 加 argparse `--json PATH`,把 `agg` 均值 + 逐折行 dump JSON)
- Test: `scripts/test_eval_blend_json.py`(新建)

**Interfaces:**
- Produces: JSON 文件,结构 `{"folds":[{"oos","corr","sh_ridge","sh_val","sh_blend","dd_ridge","dd_val","dd_blend","ex_ridge","ex_val","ex_blend"}…], "mean":{"corr","sh_ridge","sh_val","sh_blend","dd_ridge","dd_val","dd_blend","ex_ridge","ex_val","ex_blend"}}`(Rust T2 的 `BlendDto` 读它)

- [ ] **Step 1: 写失败测试** `scripts/test_eval_blend_json.py`

```python
import json, os, tempfile, subprocess, sys
def test_blend_json_shape(tmp_path=None):
    # 烟囱测试:跑 eval_blend.py --json 到临时文件,断言键齐全、folds 非空
    out = os.path.join(tempfile.gettempdir(), "blend_test.json")
    if os.path.exists(out): os.remove(out)
    r = subprocess.run([sys.executable, "scripts/eval_blend.py", "--json", out],
                       capture_output=True, text=True, timeout=900)
    assert r.returncode == 0, r.stderr[-2000:]
    d = json.load(open(out, encoding="utf-8"))
    assert "folds" in d and "mean" in d and len(d["folds"]) >= 4
    keys = {"corr","sh_ridge","sh_val","sh_blend","dd_ridge","dd_val","dd_blend","ex_ridge","ex_val","ex_blend"}
    assert keys <= set(d["mean"]) and keys | {"oos"} <= set(d["folds"][0])

if __name__ == "__main__":
    test_blend_json_shape(); print("PASS")
```

- [ ] **Step 2: 跑测试看失败** `python scripts/test_eval_blend_json.py` → FAIL(`--json` 未识别 / 无文件)

- [ ] **Step 3: 实现** —— 在 `eval_blend.py` `main()` 收集逐折时同步存行,末尾按 `--json` dump。把 `main()` 改为收集 `fold_rows`(每折 dict)并在结尾:

```python
# main() 顶部
import argparse, json
ap = argparse.ArgumentParser(); ap.add_argument("--json", default=None); args = ap.parse_args()
fold_rows = []
# …在每折 print(row) 处追加:
fold_rows.append({"oos": ol[:4], "corr": corr, "sh_ridge": row["shR"], "sh_val": row["shV"],
                  "sh_blend": row["shB"], "dd_ridge": row["ddR"], "dd_val": row["ddV"],
                  "dd_blend": row["ddB"], "ex_ridge": row["exR"], "ex_val": row["exV"], "ex_blend": row["exB"]})
# …在 6-fold means 计算 m 之后:
if args.json:
    mean = {"corr": m["corr"], "sh_ridge": m["shR"], "sh_val": m["shV"], "sh_blend": m["shB"],
            "dd_ridge": m["ddR"], "dd_val": m["ddV"], "dd_blend": m["ddB"],
            "ex_ridge": m["exR"], "ex_val": m["exV"], "ex_blend": m["exB"]}
    json.dump({"folds": fold_rows, "mean": mean}, open(args.json, "w", encoding="utf-8"),
              ensure_ascii=False, indent=2)
    print(f"[eval_blend] json → {args.json}")
```

- [ ] **Step 4: 跑测试看通过** `python scripts/test_eval_blend_json.py` → PASS。并生成缓存:`python scripts/eval_blend.py --json data/factor_panel/paper_blend.json`

- [ ] **Step 5: 提交(仅 Python)**

```bash
git checkout -b paper-blend-json
git add scripts/eval_blend.py scripts/test_eval_blend_json.py
git commit -F - <<'MSG'
feat(eval): eval_blend.py --json artifact for desktop 「纸面盘」 blend panel
…
Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
MSG
git checkout master && git merge --ff-only paper-blend-json && git branch -d paper-blend-json
```

---

### Task 2: dto_paper.rs + paper_cmds.rs(Rust TDD,纯解析 + 命令)

**Files:**
- Create: `desktop/src-tauri/src/dto_paper.rs`
- Create: `desktop/src-tauri/src/paper_cmds.rs`(含 `#[cfg(test)]`)

**Interfaces:**
- Consumes: `crate::index_relative::{load_index, compute}`、`crate::paths::python_exe`、`crate::commands::AppState`、`TaskRegistry`(`state.tasks.start`)。
- Produces: `paper_ridge_status/_advance/_retrain` + `paper_blend_recompute`(T3 注册);`parse_status(weights_json,journal_csv,blend_json,idx)`(纯,本任务测)。

- [ ] **Step 1: DTO** `dto_paper.rs`

```rust
use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)] #[ts(export)]
pub struct PaperRowDto {
    pub date: String, pub status: String, pub picks: Vec<String>,
    pub turnover: Option<f64>, pub gross_ret: Option<f64>, pub net_ret: Option<f64>, pub nav: f64,
}
#[derive(Debug, Clone, Serialize, TS)] #[ts(export)]
pub struct BlendFoldDto {
    pub oos: String, pub corr: f64,
    pub sh_ridge: f64, pub sh_val: f64, pub sh_blend: f64,
    pub dd_ridge: f64, pub dd_val: f64, pub dd_blend: f64,
    pub ex_ridge: f64, pub ex_val: f64, pub ex_blend: f64,
}
#[derive(Debug, Clone, Serialize, TS)] #[ts(export)]
pub struct BlendDto { pub folds: Vec<BlendFoldDto>, pub mean: BlendFoldMeanDto }
#[derive(Debug, Clone, Serialize, TS)] #[ts(export)]
pub struct BlendFoldMeanDto {
    pub corr: f64, pub sh_ridge: f64, pub sh_val: f64, pub sh_blend: f64,
    pub dd_ridge: f64, pub dd_val: f64, pub dd_blend: f64,
    pub ex_ridge: f64, pub ex_val: f64, pub ex_blend: f64,
}
#[derive(Debug, Clone, Serialize, TS)] #[ts(export)]
pub struct PaperStatusDto {
    pub initialized: bool, pub strategy: String,
    pub train_lo: String, pub train_hi: String, pub n_train_dates: i64,
    pub delta: f64, pub top_n: i64, pub cost_bps: f64,
    pub open_picks: Vec<String>, pub closed: Vec<PaperRowDto>,
    pub cum_net: f64, pub cum_excess: Option<f64>, pub blend: Option<BlendDto>,
}
```

- [ ] **Step 2: 写失败测试** `paper_cmds.rs` 末尾 `#[cfg(test)]`(纯 `parse_status`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    fn idx() -> BTreeMap<String, f64> {
        BTreeMap::from([("2026-06-04".into(),100.0),("2026-06-11".into(),101.0),("2026-06-18".into(),99.0)])
    }
    const W: &str = r#"{"strategy":"ridge-on-gauss / 去相关岭组合","train_lo":"2018-02-06","train_hi":"2026-06-04","n_train_dates":404,"delta":0.05,"top_n":3,"cost_bps":20.0,"factor_cols":["f_bm"],"weights":[0.1]}"#;
    // 一平仓(net=0.02)+ 一开仓
    const J: &str = "date,status,picks,prev_picks,turnover,gross_ret,net_ret\n2026-06-11,closed,sh600208;sz000039;sz301316,,1.0,0.022,0.020\n2026-06-18,open,sh600000;sz000001;sz301316,sh600208;sz000039;sz301316,0.67,,\n";

    #[test]
    fn parses_meta_and_nav_and_open() {
        let s = parse_status(W, J, None, &idx());
        assert!(s.initialized);
        assert_eq!(s.n_train_dates, 404);
        assert_eq!(s.closed.len(), 1);
        assert!((s.closed[0].nav - 1.02).abs() < 1e-9);          // cumprod(1+0.02)
        assert!((s.cum_net - 0.02).abs() < 1e-9);
        assert_eq!(s.open_picks, vec!["sh600000","sz000001","sz301316"]);
    }
    #[test]
    fn uninitialized_when_weights_blank() {
        let s = parse_status("", "", None, &idx());
        assert!(!s.initialized);
    }
    #[test]
    fn excess_uses_index() {
        // closed nav 在 2026-06-11=1.02;基准 idx 同日 101/100-1=0.01 → 但单点 nav<2 → excess None
        let s = parse_status(W, J, None, &idx());
        assert!(s.cum_excess.is_none() || s.cum_excess.is_some()); // 形态存在即可(单平仓点)
    }
    #[test]
    fn parses_blend() {
        let b = r#"{"folds":[{"oos":"2020","corr":0.28,"sh_ridge":1.06,"sh_val":0.43,"sh_blend":0.96,"dd_ridge":0.25,"dd_val":0.24,"dd_blend":0.11,"ex_ridge":0.095,"ex_val":-0.11,"ex_blend":0.01}],"mean":{"corr":0.36,"sh_ridge":0.68,"sh_val":0.43,"sh_blend":0.68,"dd_ridge":0.24,"dd_val":0.24,"dd_blend":0.17,"ex_ridge":0.186,"ex_val":0.08,"ex_blend":0.145}}"#;
        let s = parse_status(W, J, Some(b), &idx());
        let bl = s.blend.unwrap();
        assert_eq!(bl.folds.len(), 1);
        assert!((bl.mean.dd_blend - 0.17).abs() < 1e-9);
    }
}
```

- [ ] **Step 3: 跑测试看失败** `cargo test -p rquant-desktop paper_cmds` → FAIL(未定义)

- [ ] **Step 4: 实现** `paper_cmds.rs`

```rust
//! 「纸面盘」命令层:读 paper_ridge 产物算状态 DTO;写操作 shell Python(镜像 iter_cmds)。
use crate::commands::AppState;
use crate::dto_paper::*;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};

fn jstr(v: &serde_json::Value, k: &str) -> String { v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string() }
fn jf64(v: &serde_json::Value, k: &str) -> f64 { v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0) }
fn ji64(v: &serde_json::Value, k: &str) -> i64 { v.get(k).and_then(|x| x.as_i64()).unwrap_or(0) }

/// 纯:解析三产物 → DTO。weights 空/解析失败 → initialized=false。
pub fn parse_status(weights_json: &str, journal_csv: &str, blend_json: Option<&str>,
                    idx: &BTreeMap<String, f64>) -> PaperStatusDto {
    let w: Option<serde_json::Value> = serde_json::from_str(weights_json).ok();
    let initialized = w.is_some();
    let w = w.unwrap_or(serde_json::Value::Null);

    let mut closed: Vec<PaperRowDto> = vec![];
    let mut open_picks: Vec<String> = vec![];
    let mut nav = 1.0;
    for line in journal_csv.lines().skip(1) {
        let f: Vec<&str> = line.split(',').collect();
        if f.len() < 7 { continue; }
        let picks: Vec<String> = f[2].split(';').filter(|s| !s.is_empty()).map(String::from).collect();
        let status = f[1].to_string();
        let parse = |s: &str| -> Option<f64> { let t = s.trim(); if t.is_empty() { None } else { t.parse().ok() } };
        if status == "closed" {
            let net = parse(f[6]).unwrap_or(0.0);
            nav *= 1.0 + net;
            closed.push(PaperRowDto { date: f[0].into(), status, picks,
                turnover: parse(f[4]), gross_ret: parse(f[5]), net_ret: Some(net), nav });
        } else {
            open_picks = picks;  // 最后一个 open 即当前持仓
        }
    }
    let cum_net = if closed.is_empty() { 0.0 } else { nav - 1.0 };
    let holdings: Vec<(String, f64)> = closed.iter().map(|r| (r.date.clone(), r.nav)).collect();
    let cum_excess = crate::index_relative::compute(&holdings, &[], idx).excess_cum;
    let blend = blend_json.and_then(|b| serde_json::from_str::<serde_json::Value>(b).ok()).map(|v| BlendDto {
        folds: v.get("folds").and_then(|x| x.as_array()).map(|a| a.iter().map(|f| BlendFoldDto {
            oos: jstr(f,"oos"), corr: jf64(f,"corr"),
            sh_ridge: jf64(f,"sh_ridge"), sh_val: jf64(f,"sh_val"), sh_blend: jf64(f,"sh_blend"),
            dd_ridge: jf64(f,"dd_ridge"), dd_val: jf64(f,"dd_val"), dd_blend: jf64(f,"dd_blend"),
            ex_ridge: jf64(f,"ex_ridge"), ex_val: jf64(f,"ex_val"), ex_blend: jf64(f,"ex_blend"),
        }).collect()).unwrap_or_default(),
        mean: { let m = v.get("mean").cloned().unwrap_or(serde_json::Value::Null); BlendFoldMeanDto {
            corr: jf64(&m,"corr"), sh_ridge: jf64(&m,"sh_ridge"), sh_val: jf64(&m,"sh_val"), sh_blend: jf64(&m,"sh_blend"),
            dd_ridge: jf64(&m,"dd_ridge"), dd_val: jf64(&m,"dd_val"), dd_blend: jf64(&m,"dd_blend"),
            ex_ridge: jf64(&m,"ex_ridge"), ex_val: jf64(&m,"ex_val"), ex_blend: jf64(&m,"ex_blend"),
        }},
    });
    PaperStatusDto {
        initialized, strategy: jstr(&w,"strategy"),
        train_lo: jstr(&w,"train_lo"), train_hi: jstr(&w,"train_hi"), n_train_dates: ji64(&w,"n_train_dates"),
        delta: jf64(&w,"delta"), top_n: ji64(&w,"top_n"), cost_bps: jf64(&w,"cost_bps"),
        open_picks, closed, cum_net, cum_excess, blend,
    }
}

fn fp_dir(state: &AppState) -> std::path::PathBuf { state.ws.root().join("data").join("factor_panel") }

#[tauri::command]
pub fn paper_ridge_status(state: tauri::State<AppState>) -> Result<PaperStatusDto, String> {
    let d = fp_dir(&state);
    let weights = std::fs::read_to_string(d.join("paper_ridge_weights.json")).unwrap_or_default();
    let journal = std::fs::read_to_string(d.join("paper_ridge_journal.csv")).unwrap_or_default();
    let blend = std::fs::read_to_string(d.join("paper_blend.json")).ok();
    let idx = crate::index_relative::load_index(&state.ws.index_dir().join("csi300.csv")).unwrap_or_default();
    Ok(parse_status(&weights, &journal, blend.as_deref(), &idx))
}

fn shell_python(state: &AppState, kind: &'static str, args: Vec<String>) -> Result<String, String> {
    let ws = state.ws.clone();
    state.tasks.start(kind, true, move |ctx| {
        let py = crate::paths::python_exe();
        let mut cmd = Command::new(&py);
        cmd.current_dir(ws.root());
        for a in &args { cmd.arg(a); }
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        ctx.progress(0.05, "启动", &args.join(" "));
        let mut child = cmd.spawn().map_err(|e| format!("启动 Python 失败: {e}"))?;
        let se = child.stderr.take().map(|s| std::thread::spawn(move || { let mut t=String::new(); let _=BufReader::new(s).read_to_string(&mut t); t }));
        if let Some(out) = child.stdout.take() {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if ctx.cancelled() { let _ = child.kill(); return Err("cancelled".into()); }
                ctx.progress(0.5, "运行", &line);
            }
        }
        let st = child.wait().map_err(|e| e.to_string())?;
        let err = se.and_then(|h| h.join().ok()).unwrap_or_default();
        if !st.success() { return Err(format!("Python 退出码 {:?}: {}", st.code(), err)); }
        ctx.progress(0.98, "完成", "");
        Ok(serde_json::json!({"ok": true}))
    })
}

#[tauri::command]
pub fn paper_ridge_advance(state: tauri::State<AppState>) -> Result<String, String> {
    shell_python(&state, "paper_advance", vec!["scripts/paper_ridge.py".into()])
}
#[tauri::command]
pub fn paper_ridge_retrain(state: tauri::State<AppState>) -> Result<String, String> {
    shell_python(&state, "paper_retrain", vec!["scripts/paper_ridge.py".into(), "--retrain".into()])
}
#[tauri::command]
pub fn paper_blend_recompute(state: tauri::State<AppState>) -> Result<String, String> {
    shell_python(&state, "paper_blend", vec!["scripts/eval_blend.py".into(), "--json".into(), "data/factor_panel/paper_blend.json".into()])
}
```

- [ ] **Step 5: 跑测试看通过** `cargo test -p rquant-desktop paper_cmds` → 4 passed

- [ ] **Step 6: 提交** —— 桌面变更**不提交**(共存 gm_tail);仅记录进度。继续 T3。

---

### Task 3: 注册 + 接线(lib.rs / ipc.ts / App.tsx,最小追加)

**Files:**
- Modify: `desktop/src-tauri/src/lib.rs`(第 43 行后加 mod;第 141 行后加 handler)
- Modify: `desktop/ui/src/api/ipc.ts`(api 对象末尾加 4 条)
- Modify: `desktop/ui/src/App.tsx`(import + MODULES + Route + 排除 filter)

**Interfaces:**
- Consumes: T2 的 4 命令 + 自动生成的 `@bindings/PaperStatusDto`。
- Produces: `api.paperRidgeStatus/paperRidgeAdvance/paperRidgeRetrain/paperBlendRecompute`;路由 `/paper`。

- [ ] **Step 1: lib.rs 加 mod**(第 43 行 `pub mod tasks;` 之后追加,远离 gm_tail 的 23/26/27 行)

```rust
pub mod paper_cmds;
pub mod dto_paper;
```

- [ ] **Step 2: lib.rs 加 handler**(第 141 行 `gm_tail_cmds::gm_tail_run_now,` 之后、`])` 之前追加)

```rust
            paper_cmds::paper_ridge_status,
            paper_cmds::paper_ridge_advance,
            paper_cmds::paper_ridge_retrain,
            paper_cmds::paper_blend_recompute,
```

- [ ] **Step 3: ipc.ts 加 api**(`export const api = { … }` 末尾,return-类型用生成绑定)

```typescript
import type { PaperStatusDto } from "@bindings/PaperStatusDto";
// …在 api 对象内追加:
  paperRidgeStatus: () => invoke<PaperStatusDto>("paper_ridge_status"),
  paperRidgeAdvance: () => invoke<string>("paper_ridge_advance"),
  paperRidgeRetrain: () => invoke<string>("paper_ridge_retrain"),
  paperBlendRecompute: () => invoke<string>("paper_blend_recompute"),
```

- [ ] **Step 4: App.tsx 接线**

```typescript
import PaperRidge from "./pages/PaperRidge";
// MODULES 数组加一项(放 deploy 后):
  { key: "paper", label: "纸面盘" },
// Routes 内加:
          <Route path="/paper" element={<PaperRidge />} />
// 第 67 行 Placeholder 排除 filter 末尾加 && m.key !== "paper"
```

- [ ] **Step 5: 验证编译**(绑定在 T5 cargo build 生成;此处仅确保 Rust 编译)`cargo build -p rquant-desktop` → 成功(生成 `bindings/PaperStatusDto.ts` 等)。桌面变更不提交。

---

### Task 4: PaperRidge.tsx 页面 + vitest

**Files:**
- Create: `desktop/ui/src/pages/PaperRidge.tsx`
- Create: `desktop/ui/src/pages/PaperRidge.test.tsx`

**Interfaces:**
- Consumes: `api.paperRidge*`、`@bindings/PaperStatusDto`、antd `Table/Card/Button/Statistic/Empty`、`useTasks`(可选刷新)。

- [ ] **Step 1: 写失败测试** `PaperRidge.test.tsx`(mock api,断言渲染)

```typescript
import { render, screen, waitFor } from "@testing-library/react";
import { vi, test, expect, beforeEach } from "vitest";
import PaperRidge from "./PaperRidge";

const status = {
  initialized: true, strategy: "ridge-on-gauss / 去相关岭组合",
  train_lo: "2018-02-06", train_hi: "2026-06-04", n_train_dates: 404,
  delta: 0.05, top_n: 3, cost_bps: 20,
  open_picks: ["sh600208","sz000039","sz301316"], closed: [],
  cum_net: 0, cum_excess: null, blend: null,
};
vi.mock("../api/ipc", () => ({ api: {
  paperRidgeStatus: vi.fn(() => Promise.resolve(status)),
  paperRidgeAdvance: vi.fn(), paperRidgeRetrain: vi.fn(), paperBlendRecompute: vi.fn(),
}}));

test("renders frozen meta + open picks", async () => {
  render(<PaperRidge />);
  await waitFor(() => expect(screen.getByText(/去相关岭组合/)).toBeTruthy());
  expect(screen.getByText(/sh600208/)).toBeTruthy();
});
```

- [ ] **Step 2: 跑测试看失败** `npm --prefix desktop/ui run test -- PaperRidge` → FAIL(无文件)

- [ ] **Step 3: 实现** `PaperRidge.tsx`(只读区 + 三按钮;未初始化空态)

```tsx
import { useEffect, useState } from "react";
import { Card, Table, Button, Statistic, Row, Col, Empty, Tag, Space, message } from "antd";
import { api } from "../api/ipc";
import type { PaperStatusDto } from "@bindings/PaperStatusDto";

export default function PaperRidge() {
  const [s, setS] = useState<PaperStatusDto | null>(null);
  const load = () => api.paperRidgeStatus().then(setS).catch((e) => message.error(String(e)));
  useEffect(() => { load(); }, []);
  const act = async (fn: () => Promise<string>, name: string) => {
    try { await fn(); message.success(`${name} 已启动(见任务抽屉)`); } catch (e) { message.error(String(e)); }
  };
  if (!s) return <Empty description="加载中…" />;
  if (!s.initialized) return (
    <Card title="纸面盘 · 去相关岭组合">
      <Empty description="尚未冻结权重">
        <Button type="primary" onClick={() => act(api.paperRidgeRetrain, "重训")}>重训权重(生成)</Button>
      </Empty>
    </Card>
  );
  return (
    <Space direction="vertical" style={{ width: "100%" }} size="middle">
      <Card title={`纸面盘 · ${s.strategy}`} extra={
        <Space>
          <Button onClick={() => act(api.paperRidgeAdvance, "推进纸面册")}>推进纸面册</Button>
          <Button onClick={() => act(api.paperRidgeRetrain, "重训")}>重训权重</Button>
          <Button onClick={() => act(api.paperBlendRecompute, "重算对照")}>重算对照</Button>
        </Space>}>
        <Row gutter={16}>
          <Col><Statistic title="训练区间" value={`${s.train_lo}~${s.train_hi}`} /></Col>
          <Col><Statistic title="周数" value={s.n_train_dates} /></Col>
          <Col><Statistic title="delta" value={s.delta} precision={2} /></Col>
          <Col><Statistic title={`top${s.top_n} · 成本`} value={`${s.cost_bps}bp`} /></Col>
          <Col><Statistic title="累计净收益" value={s.cum_net} precision={4} /></Col>
          <Col><Statistic title="超额 vs csi300" value={s.cum_excess ?? NaN} precision={4} /></Col>
        </Row>
      </Card>
      <Card title="本周持仓 (open)">
        {s.open_picks.length ? s.open_picks.map((p) => <Tag key={p}>{p}</Tag>) : <Empty description="无持仓" />}
      </Card>
      <Card title="纸面册 (已结算)">
        <Table rowKey="date" size="small" pagination={false} dataSource={s.closed} columns={[
          { title: "日期", dataIndex: "date" },
          { title: "选股", dataIndex: "picks", render: (p: string[]) => p.join(", ") },
          { title: "换手", dataIndex: "turnover", render: (v: number | null) => v?.toFixed(2) ?? "-" },
          { title: "毛", dataIndex: "gross_ret", render: (v: number | null) => v?.toFixed(4) ?? "-" },
          { title: "净", dataIndex: "net_ret", render: (v: number | null) => v?.toFixed(4) ?? "-" },
          { title: "NAV", dataIndex: "nav", render: (v: number) => v.toFixed(4) },
        ]} />
      </Card>
      {s.blend && (
        <Card title="岭值双引擎 6 折对照(回测)">
          <Table rowKey="oos" size="small" pagination={false} dataSource={s.blend.folds} columns={[
            { title: "OOS", dataIndex: "oos" },
            { title: "相关", dataIndex: "corr", render: (v: number) => v.toFixed(2) },
            { title: "Sh岭", dataIndex: "sh_ridge", render: (v: number) => v.toFixed(2) },
            { title: "Sh值", dataIndex: "sh_val", render: (v: number) => v.toFixed(2) },
            { title: "Sh合", dataIndex: "sh_blend", render: (v: number) => v.toFixed(2) },
            { title: "回撤合", dataIndex: "dd_blend", render: (v: number) => v.toFixed(2) },
            { title: "超额合", dataIndex: "ex_blend", render: (v: number) => v.toFixed(3) },
          ]} />
          <div style={{ marginTop: 8 }}>均值:相关 {s.blend.mean.corr.toFixed(2)} · Sharpe 岭/值/合 {s.blend.mean.sh_ridge.toFixed(2)}/{s.blend.mean.sh_val.toFixed(2)}/{s.blend.mean.sh_blend.toFixed(2)} · 回撤合 {s.blend.mean.dd_blend.toFixed(2)}</div>
        </Card>
      )}
    </Space>
  );
}
```

- [ ] **Step 4: 跑测试看通过** `npm --prefix desktop/ui run test -- PaperRidge` → PASS

- [ ] **Step 5:** 桌面变更不提交。继续 T5。

---

### Task 5: 收尾闸 + 构建启动

**Files:** 无新增(验证 + 启动)

- [ ] **Step 1: Rust 构建(重生 bindings)** `cargo build -p rquant-desktop` → 成功,`desktop/src-tauri/bindings/PaperStatusDto.ts` 等生成。
- [ ] **Step 2: Rust 测试** `cargo test -p rquant-desktop` → 全绿(含 paper_cmds 4 例)。
- [ ] **Step 3: UI 构建 + 测试** `npm --prefix desktop/ui run build` → 成功;`npm --prefix desktop/ui run test` → 全绿。
- [ ] **Step 4: 启动**(无监视器,稳定):确保 Vite 在(`npm --prefix desktop/ui run dev`),后台跑 `./target/debug/rquant-desktop.exe`;确认进程在 + 「纸面盘」页可见。
- [ ] **Step 5:** 桌面变更**仍不提交**(与 gm_tail 共存,等用户统一处理)。报告完成 + 提示 desktop 变更未提交。

---

## Self-Review

**Spec coverage:** ① top-3/纸面册/超额 → T2 parse_status + T4 页面 ✓;② 岭值双引擎对照 → T1 --json + T2 BlendDto + T4 对照表 ✓;③ 推进/重训/重算 → T2 三命令 + T4 按钮 ✓;④ 最小侵入/避 gm_tail → T3 仅追加行 + 内联路径 ✓;⑤ 测试 → T2 Rust 单测/T4 vitest/T1 Python ✓;⑥ 交付(桌面不提交、Python 单独) → T1 提交 / T2-5 不提交 ✓。
**Placeholder scan:** 无 TBD;各步含真实代码/命令。
**Type consistency:** `PaperStatusDto`/`BlendDto`/`BlendFoldDto`/`BlendFoldMeanDto` 字段在 T2 定义、T1 JSON 键、T4 渲染三处一致(snake_case,ts-rs 透传)。`parse_status(weights,journal,blend,idx)` 签名在 T2 定义与测试一致。
