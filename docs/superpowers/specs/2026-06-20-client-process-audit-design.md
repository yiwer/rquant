# 客户端流程审计 + 可观测/健壮性加固 设计(process-audit)

> 状态:已 brainstorm 定稿,待 writing-plans。日期:2026-06-20。
> 前序:sub-1/2a/3a + task-ux 已合入 master;含 cwd 修复(09ec028)、tree_list 去噪(f65d448)。本项复用其桌面范式。

## 0. 背景与问题(三路审查实测)

用户要求"检查健壮性与日志流程信息打印完整性,并建立完整的流程审计展示在客户端"。审查发现:

**健壮性**(2 Critical / 6 Important / 6 Minor):
- C1 `deploy_cmds.rs:189` `deploy_commit_month` 同步在 IPC 线程跑全量 `run_screen`、无 `catch_unwind` → 冻结所有命令 + panic 崩 IPC。
- C2 `tasks.rs`(43/99/143/153/163)`.lock().expect("task map poisoned")` → 一次持锁 panic 即**永久毒化**任务子系统。
- I1 `commands.rs:66` `cockpit_overview` 同步 IPC + 子进程 + 无 panic 守卫;I2 `commands.rs:53` `book_detail` 损坏状态 `.ok().flatten()` 静默吞成空快照;I3 `readers.rs:28` trip 序列化失败静默置 Null;I4 `screen_cmds.rs:64,91,92` 坏日期 `.ok()` 静默变 None(用最新日跑错);I5 `iter_cmds.rs:87-95` stderr 在 `wait()` 后读 → 管道死锁风险 + 仅留末行;I6 `deploy_cmds.rs:14/analyze_cmds.rs:17,45` kday/index CSV 硬编码列号(schema 变即静默算错)。

**日志/留痕完整性**:`tauri_plugin_log` 仅 `::new().build()` → 不落盘、全桥零 `log::` 调用;`TaskRegistry` 纯内存重启即丢;各命令 `ctx.progress` 粗、**参数/触及文件/计数/耗时不留痕**;`deploy_commit_month` 全程静默;引擎内调用黑盒。

**客户端可观测**:零散持久面(回测 runs/选股回测 runs/研究 ledger/部署月表/run.log tail)但**无统一审计时间线**;指定日选股/因子/manual-run/deploy 无持久运行记录;失败任务**完整错误不展示**;无"触及了哪些文件"。

## 1. 决策(brainstorm 定论)

| 决策 | 结论 |
|---|---|
| 审计深度 | **全量审计日志(JSONL,跨会话持久)+ 新顶层「审计」页**(时间线+筛选/检索+详情:参数/阶段时序/触及文件/完整错误+shell stderr/产物指针/结果摘要)。各命令补 `log::info` 关键事实。 |
| 捕获机制 | **TaskRegistry 单一卡点**(方案 A):所有可审计操作走 `tasks.start`(`deploy_commit`、`eval_certify` 改任务,顺带修 C1);registry 在终态组装 `AuditRecord` 追加落盘。 |
| 健壮性 | 随本项修 **Critical + Important**(2+6);Minor(6)记入审计页发现、后续。 |

## 2. 架构

### 2.1 审计捕获(后端)
- 新 `audit.rs`:`AuditRecord` 模型 + `append(ws, &rec)`(`OpenOptions` append 单行写 `.rquant-desktop/audit/audit.jsonl`,gitignored)+ `read(ws, limit, filter) -> Vec<AuditRecord>`(读尾部 N、按 kind/status 过滤,纯逻辑可 TDD)。
- `tasks.rs` 改造:
  - `start(kind:&str, heavy:bool, params: serde_json::Value, body)` 加 `params`。registry 记 `started_at`(SystemTime)+ start `Instant`;每次 `ctx.progress` 追加 `AuditStage{stage,detail,at_ms}`(at_ms = 距 start 的毫秒);`ctx.note_file(path)` 累加触及文件;`ctx.note_summary(&str)` 可选设结果摘要。
  - 终态(done/failed/cancelled/panic)组装 `AuditRecord{id,kind,params,started_at,ended_at,duration_ms,stages,files,status,error,result_summary,artifact}` → `audit::append`。错误含 shell `cmdline`+全 stderr(由命令体经 `note_*`/error 提供)。
- `lib.rs`:`tauri_plugin_log` 加文件 target(`.rquant-desktop/logs/`,引擎自身 `log::` 随之落盘);各命令体补 `log::info!`(参数/解析后路径/计数/耗时)。

### 2.2 命令侧
- 每个重命令在 `tasks.start` 传 `params`(其输入 JSON:config/universe/as_of/top/window/from/to/factors/books/...),并在体内 `ctx.note_file(...)` 声明桥层可知的关键输入/产物(config、universe、index、kday_dir、输出 run/report 路径——非逐股 CSV,诚实标注"桥层输入");shell-out(iter)记 `cmdline`+并发读全 stderr(修 I5)。
- `deploy_commit_month`、`eval_certify` 改走 `tasks.start`(heavy/light 适配)→ 自动审计 + 修 C1。

### 2.3 DTO(`dto_audit.rs`,`#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]`)
- `AuditStageDto { stage:String, detail:String, at_ms:f64 }`
- `AuditRecordDto { id:String, kind:String, params:serde_json::Value, started_at:String, ended_at:String, duration_ms:f64, stages:Vec<AuditStageDto>, files:Vec<String>, status:String, error:Option<String>, result_summary:Option<String>, artifact:Option<String> }`
- 数字用 `f64`(ts-rs→TS `number`,避开 i64→bigint);名全局唯一。

### 2.4 命令
- `audit_list(limit:u32) -> Vec<AuditRecordDto>`(新→旧)。
- `audit_log_tail(lines:u32) -> String`(原始桥日志文件尾部,仿 `runlog_tail`)。

## 3. 客户端

- 新顶层 **`审计`** 页(`pages/Audit.tsx`)+ `stores/audit.ts` + `api/ipc` 三 wrapper + `App.tsx` 路由/`labels` + 占位过滤排除。
- **时间线表**:时间 · 类型(中文 `kindZh`)· 状态徽标 · 耗时 · 参数摘要(1 行)· 错误预览;antd `Table` + 按 类型/状态 `Select` 筛选 + 文本 `Input` 检索(前端过滤)。
- **详情抽屉**(点行):完整参数(JSON 折叠/键值表)· **阶段时序**(stage·detail·距开始 `at_ms`)· 触及文件列表 · **完整错误**(含 shell stderr/cmdline,可复制)· 结果摘要 · 产物跳转(若 artifact 指向 run/report → 跳对应页)。
- 可选"原始日志"tab:`audit_log_tail` 展示桥日志(`<pre>`,仿 Cockpit run.log)。
- 现有 `TaskDrawer`/`TaskRunning` 仍管在途实时;审计页 = 终态落盘后的事后历史。

## 4. 数据流

- 跑任务:命令 `tasks.start(kind, heavy, params, body)` → registry 记 started/stages/files → 终态组装 `AuditRecord` append JSONL(+ `log::info` 落日志文件)。
- 看审计:`审计`页 → `audit_list(200)` → 时间线;点行 → 详情抽屉(记录已含全字段,无需二次取);"原始日志" → `audit_log_tail`。
- 跨会话:JSONL 持久 → 重启后历史仍在(与内存 TaskDrawer 互补)。

## 5. 错误处理(诚实)

- 审计**记录失败不得影响主流程**:`audit::append` 出错仅 `log::warn`,不 `?` 冒泡毁任务结果(审计是旁路)。
- 失败任务的**完整错误**进审计记录并在详情全文展示(不再只红"失败"标);shell-out 全 stderr + cmdline 入 error。
- C2 后 mutex 用 `into_inner` 恢复中毒;I2/I4 等把静默失败改为显式错误透出。
- 文件触及为"桥层可知输入/产物",详情页注明非逐股级血缘(不臆造完整性)。

## 6. 健壮性修复(随本项,Crit+Imp,每条带回归)

C1 deploy_commit→`tasks.start`;C2 `tasks.rs` 五处 `.lock()` 用 `unwrap_or_else(|p| p.into_inner())`;I1 `cockpit_overview` `catch_unwind` 守卫(panic 返降级 DTO);I2 `book_detail` 损坏状态透出 corrupt(非空快照);I3 trip 序列化失败 `log::warn` 保留兜底;I4 `screen_asof/backtest` 坏日期返 `Err`;I5 iter stderr 并发读(线程/`output()`)治死锁 + 全量;I6 kday/index CSV 按表头列名解析(或首行校验)。

## 7. 测试

- Rust:`audit.rs` 纯逻辑 TDD(记录组装序列化往返、`read` 尾部 N + kind/status 过滤);`tasks.rs` 任务→审计落盘单测(start 传 params→done/failed 后 JSONL 有正确记录、stages 有时序、files 收集);健壮性回归(坏日期→Err、损坏状态→corrupt 透出、mutex 中毒后 `into_inner` 恢复、CSV 列名解析、iter stderr 全量)。
- 前端:vitest `stores/audit.ts`(注入 mock api,list/筛选)+ `Audit.tsx` 渲染(时间线行/详情抽屉/筛选,注入 mock)。
- 收尾:`cargo test --workspace` + `tsc`/`vitest`/`build` 全绿;GUI 冒烟(release):跑一次选股→审计页出现该记录(参数/阶段时序/耗时/触及文件齐全)、故意坏日期→详情见完整 Err、原始日志 tab 有内容。

## 8. 范围边界(YAGNI)

不含:逐股级文件血缘 / 引擎逐 bar 留痕;日志检索引擎(文件 tail + 结构化记录前端筛选即可);审计记录轮转/清理 UI(JSONL 追加,体量小;后续需要再加);Minor 健壮项(6)本项不修(记审计页发现);不改后端引擎进度粒度(沿用现阶段 + 审计阶段时序补足感知)。
