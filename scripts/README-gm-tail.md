# gm 尾盘取数 + 定时任务（可移植）

盘中 14:46 用掘金 `current()` 拉全市场广度 → 漏斗筛短名单 → `history_n` 拉短名单 15m，
喂 `build_intraday_factors.py`（14:45-asof）。**任意实例/克隆可用**：路径运行时解析，无写死。

## 组成

| 件 | 作用 |
|---|---|
| `scripts/fetch_gm_realtime.py` | 取数器（smoke/bench/snapshot/tail 四模式）。`REPO` 自 `__file__` 解析 |
| `scripts/build_gm_shortlist.py` | 漏斗：快照 → 门槛 + 粗排 → 短名单 |
| `scripts/build_gm_daily_pool.py` | 日线层(kday+可选财务) → 候选池 `daily_pool.txt`（隔夜跑，喂 `pool`） |
| `scripts/gm_tail_run.ps1` | **可移植** launcher：`$repo=Split-Path $PSScriptRoot`、`$py=$env:RQUANT_PYTHON??python`；读 `data/gm/tail.config.json` 构造参数 |
| `desktop/src-tauri/src/gm_tail.rs` + `gm_tail_cmds.rs` | app 内定时任务模块：install/remove/run_now/status/配置读写（Tauri 命令） |
| `desktop/src-tauri/src/dto_gm.rs` | `GmTailConfig` / `GmTailStatusDto`（ts-rs 导出前端类型） |
| `data/gm/tail.config.json` | **每实例配置**（排程时刻 + 漏斗旋钮）；缺失→默认值 |

数据位置全部经 `Workspace`（`paths.rs` 的 `gm_*` 方法），跟随仓库根，移植即生效。

## 新实例三步起

1. **装依赖**：`pip install gm`（掘金 SDK；有 cp313 wheel）。python 不在 PATH 时设 `RQUANT_PYTHON`。
2. **放 token**：myquant.cn 注册取 token → 存 `data/gm/.token`（已 gitignore）。
3. **装计划任务**：
   - app 内：调 `gm_tail_install`（驾驶舱按钮，未来 UI）；或
   - 手动：`schtasks /Create /TN rquant-gm-tail /TR "powershell -NoProfile -ExecutionPolicy Bypass -File \"<repo>\scripts\gm_tail_run.ps1\"" /SC WEEKLY /D MON,TUE,WED,THU,FRI /ST 14:46 /F`

## 配置 `data/gm/tail.config.json`

```json
{ "schedule_time":"14:46", "rank":"liquidity", "top":300,
  "pool":"", "min_amount":30000000, "min_price":2.0, "drop_limit_up":false }
```
- `rank`：`liquidity`(中性) / `intraday` / `range_pos` / `vwap_gap` — 你的日内偏好
- `pool`：日线层候选集文件（相对仓库根或绝对；空=不用）→ 让日线 alpha 主筛。
  隔夜生成：`python scripts/build_gm_daily_pool.py --rank liquidity --top 800`（门槛通用、`--rank` 换你偏好、`--min-roe/--min-np-yoy` 可选财务门槛）→ 写 `data/gm/daily_pool.txt`，把 `pool` 设为它即可
- `schedule_time`：改后需重新 `gm_tail_install`（schtasks 排程在任务里，不在配置里热生效）

## Tauri 命令（前端用）

`gm_tail_status` / `gm_tail_get_config` / `gm_tail_set_config` / `gm_tail_install` / `gm_tail_remove` / `gm_tail_run_now`。
`cockpit_overview` 的 `gm_tail` 字段含任务 next/last/status。

## 运维红线

- **跑数据别开 VPN/TUN 全局代理**：掘金服务器在国内，全局代理把流量绕境外→全链慢 ~10×（35s→6min），会吃掉尾盘窗口。要么 14:46 关 VPN，要么 Clash 配 `GEOIP,CN,DIRECT`。
- 计划任务**仅登录时运行**（无存密码）；交易日 14:46 须开机并登录。
- 幂等：同日重跑覆盖当日产物，无害。日志 `data/gm/tail.log`。
