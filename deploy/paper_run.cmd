@echo off
rem ============================================================
rem rquant paper-trading daily run (live drill, 2026-06-12)
rem
rem Book 1+2 (execution layer): sh600030 / sh600036 single-name
rem   60m paper-sim, regime_adaptive v4 frozen tree, commit daily.
rem Book 3 (selection layer): 10-name daily-bar universe, strength
rem   tree top-3 soft list; printed DAILY for visibility, but
rem   --commit only on MONDAYS (weekly cadence ~ backtested reb5;
rem   other days run as [DRY RUN] - holdings untouched).
rem
rem Discipline: run AFTER 15:00 session close only - sina returns
rem   forming bars intraday (pending-decision design tolerates it,
rem   but the last booked step would use non-final prices).
rem Idempotent: same-day reruns are harmless (bars_replayed=0).
rem Remove schedule: schtasks /delete /tn rquant-paper /f
rem ============================================================
chcp 65001 >nul
cd /d E:\rust-app\rquant
echo ==== %date% %time% ==== >> paper\run.log

rem ---- Books 1+2: single-name 60m execution paper-sim ----
for %%S in (sh600030 sh600036) do (
  target\release\rquant.exe signal --tree deploy\tree_v4_frozen.yaml --fetch %%S --scale 60 --adjust qfq --primary paper\p_%%S.csv --state paper\paper_%%S.json --warmup 80 --commit --out paper\sig_%%S.json >> paper\run.log 2>&1
)

rem ---- Book 3: 10-name daily universe, strength tree top-3 ----
for %%S in (sh600519 sz000858 sh600036 sh601318 sh600900 sz000333 sz300750 sh600276 sh601088 sh600030) do (
  target\release\rquant.exe fetch --symbol %%S --scale 240 --datalen 1023 --adjust qfq --out paper\pd_%%S.csv >> paper\run.log 2>&1
)
for /f %%D in ('powershell -nop -c "(Get-Date).DayOfWeek.value__"') do set DOW=%%D
set PCOMMIT=
if "%DOW%"=="1" set PCOMMIT=--commit
target\release\rquant.exe signal --tree deploy\strength_v1_frozen.yaml --universe deploy\universe_10.csv --top 3 --soft --warmup 80 --state paper\holdings_top3.json %PCOMMIT% --out paper\sig_portfolio.json >> paper\run.log 2>&1
