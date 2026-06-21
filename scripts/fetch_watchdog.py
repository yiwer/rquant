#!/usr/bin/env python3
"""看门狗：监督 fetch_baostock.py，无进展即自动 kill+resume 重启（应对 baostock 挂死/限流停滞）。

baostock 在持续负载下会挂死/停滞；socket 超时不总能解。看门狗每 POLL 秒查 k15m 文件数，
若 STALL 秒无新增则 kill 子进程并 resume 重启，使停滞代价从 ~30min 降到 ~数分钟。
完成判据：子进程自行退出且日志含 DONE。日志 → data/baostock/fetch.log。
"""
import glob, os, subprocess, sys, time

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
K15M = os.path.join(REPO, "data", "baostock", "k15m")
LOG = os.path.join(REPO, "data", "baostock", "fetch.log")
POLL = 90        # 查询间隔秒
STALL = 360      # 每个 STALL 窗口评估一次增长
MIN_GROWTH = 4   # STALL 窗口内新增 < 此 → 判停滞/限频爬行并重启（健康 ~40s/股→360s 约+9；<4=异常）
MAX_RESTARTS = 500
BACKOFF_BASE = 300   # 无进展(登录失败/配额)首次退避秒
BACKOFF_CAP = 1800   # 退避上限(30min)；baostock 配额按日重置，半小时内自动续抓


def count():
    return len(glob.glob(os.path.join(K15M, "*.csv")))


def main():
    restarts = 0
    consec_fail = 0   # 连续无进展次数（驱动指数退避）
    while restarts < MAX_RESTARTS:
        start_count = count()
        f = open(LOG, "a", encoding="utf-8")
        f.write(f"\n[watchdog] === start fetch (restart #{restarts}) count={start_count} ===\n"); f.flush()
        p = subprocess.Popen([sys.executable, os.path.join(REPO, "scripts", "fetch_baostock.py")],
                             stdout=f, stderr=subprocess.STDOUT, cwd=REPO)
        win_count, win_time = start_count, time.time()
        reason = None
        while True:
            time.sleep(POLL)
            rc = p.poll()
            if rc is not None:
                reason = "exited"; break
            now = time.time()
            if now - win_time >= STALL:  # 每窗评估增长；<MIN_GROWTH 视为停滞/爬行
                c = count()
                if c - win_count < MIN_GROWTH:
                    p.kill(); reason = f"slow(+{c - win_count}/{int(STALL)}s)"; break
                win_count, win_time = c, now
        end_count = count()
        f.write(f"[watchdog] subprocess {reason} at count={end_count}\n"); f.close()
        if reason == "exited":
            tail = open(LOG, encoding="utf-8", errors="replace").read()[-800:]
            if "DONE ok=" in tail:
                print(f"[watchdog] fetch DONE, count={end_count}", flush=True)
                break
        restarts += 1
        # 退避：本轮有进展→立即续抓(健康)；无进展(登录失败/配额/秒退)→指数退避，
        # 避免狂重启撞 baostock 配额。配额按日重置，退避上限(30min)内会自动续上。
        if end_count > start_count:
            consec_fail = 0
            backoff = 3
        else:
            consec_fail += 1
            backoff = min(BACKOFF_CAP, BACKOFF_BASE * (2 ** (consec_fail - 1)))
        msg = (f"[watchdog] {reason} (+{end_count - start_count}) → restart #{restarts}, "
               f"backoff {backoff}s (consec_fail={consec_fail})")
        print(msg, flush=True)
        with open(LOG, "a", encoding="utf-8") as lf:
            lf.write(msg + "\n")
        time.sleep(backoff)
    print(f"[watchdog] END count={count()} restarts={restarts}", flush=True)


if __name__ == "__main__":
    main()
