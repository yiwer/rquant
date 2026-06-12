@echo off
rem ============================================================
rem rquant paper-trading daily run (minimal live drill, 2026-06-12)
rem Symbols: sh600030 / sh600036 (the only strict-OS-evidence names)
rem Tree:    deploy\tree_v4_frozen.yaml (frozen copy - examples/ tree
rem          can keep evolving without breaking live paper books)
rem Discipline: run AFTER 15:00 session close only - sina returns the
rem          forming bar intraday; pending-decision design tolerates it
rem          but the last booked step would use non-final prices.
rem Idempotent: same-day reruns are harmless (bars_replayed=0).
rem Remove schedule: schtasks /delete /tn rquant-paper /f
rem ============================================================
chcp 65001 >nul
cd /d E:\rust-app\rquant
echo ==== %date% %time% ==== >> paper\run.log
for %%S in (sh600030 sh600036) do (
  target\release\rquant.exe signal --tree deploy\tree_v4_frozen.yaml --fetch %%S --scale 60 --adjust qfq --primary paper\p_%%S.csv --state paper\paper_%%S.json --warmup 80 --commit --out paper\sig_%%S.json >> paper\run.log 2>&1
)
