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
POLL = 90       # 查询间隔秒
STALL = 360     # 无新增超过此秒数 → 判停滞重启（单股最长~60s 超时，360s 不会误杀）
MAX_RESTARTS = 500


def count():
    return len(glob.glob(os.path.join(K15M, "*.csv")))


def main():
    restarts = 0
    while restarts < MAX_RESTARTS:
        f = open(LOG, "a", encoding="utf-8")
        f.write(f"\n[watchdog] === start fetch (restart #{restarts}) count={count()} ===\n"); f.flush()
        p = subprocess.Popen([sys.executable, os.path.join(REPO, "scripts", "fetch_baostock.py")],
                             stdout=f, stderr=subprocess.STDOUT, cwd=REPO)
        last, last_prog = count(), time.time()
        reason = None
        while True:
            time.sleep(POLL)
            rc = p.poll()
            c = count()
            if c > last:
                last, last_prog = c, time.time()
            if rc is not None:
                reason = "exited"; break
            if time.time() - last_prog > STALL:
                p.kill(); reason = "stall"; break
        f.write(f"[watchdog] subprocess {reason} at count={count()}\n"); f.close()
        if reason == "exited":
            tail = open(LOG, encoding="utf-8", errors="replace").read()[-800:]
            if "DONE ok=" in tail:
                print(f"[watchdog] fetch DONE, count={count()}", flush=True)
                break
            restarts += 1
            print(f"[watchdog] exited w/o DONE → restart #{restarts}", flush=True)
        else:
            restarts += 1
            print(f"[watchdog] STALL at {count()} → restart #{restarts}", flush=True)
        time.sleep(3)
    print(f"[watchdog] END count={count()} restarts={restarts}", flush=True)


if __name__ == "__main__":
    main()
