@echo off
REM Deep-history daily qfq fetch (2018-01-01..today) via Rust multi-window stitching.
REM Fetch date: 2026-06-15   Method: rquant fetch --from (tencent ~640/req window-merged + leading-trim)
REM Output: data\<symbol>.csv (gitignored). Idempotent.
setlocal
set RQ=target\release\rquant.exe
set FROM=2018-01-01
for %%S in (sh600030 sh600036 sh600276 sh600519 sh600900 sh601088 sh601318 sz000333 sz000858 sz300750) do (
  echo [fetch] %%S
  %RQ% fetch --symbol %%S --scale 240 --adjust qfq --from %FROM% --out data\%%S.csv
)
echo [done] deep fetch complete
endlocal
