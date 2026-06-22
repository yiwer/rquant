#!/usr/bin/env python3
"""生成 symbol→中文名称 映射，供桌面端选股榜显示名称。
源 = akshare stock_info_a_code_name()（全 A 股 code+name）。6位code→sh/sz/bj 前缀(对齐 universe 格式)。
产出：desktop/ui/src/data/stockNames.json（UI 静态导入）+ data/baostock/stock_names.csv（本地记录）。
名称为当前在册名（展示用，非时点）；重跑即刷新。baostock 登录不稳时用本脚本（akshare）。"""
import akshare as ak, json, os, csv, re

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def prefix(code: str) -> str:
    c0 = code[0]
    if c0 == "6":
        return "sh"           # 沪主板/科创(688)
    if c0 in "03":
        return "sz"           # 深主板(00)/创业(30)
    if c0 in "48":
        return "bj"           # 北交所(8/4)——universe 多为 sh/sz，bj 不匹配无害
    return "sz"


def main():
    df = ak.stock_info_a_code_name()
    m = {}
    for r in df.itertuples():
        code = str(r.code).zfill(6)
        name = re.sub(r"\s+", "", str(r.name))  # 去半/全角空格(如 "万  科Ａ")
        m[prefix(code) + code] = name
    os.makedirs(os.path.join(REPO, "desktop/ui/src/data"), exist_ok=True)
    json.dump(m, open(os.path.join(REPO, "desktop/ui/src/data/stockNames.json"), "w", encoding="utf-8"),
              ensure_ascii=False, separators=(",", ":"))
    os.makedirs(os.path.join(REPO, "data/baostock"), exist_ok=True)
    with open(os.path.join(REPO, "data/baostock/stock_names.csv"), "w", newline="", encoding="utf-8") as f:
        w = csv.writer(f); w.writerow(["symbol", "name"])
        for k, v in sorted(m.items()):
            w.writerow([k, v])
    # ST/*ST 高风险股列表（按名称判定，含 ST/*ST/SST/退市风险标记）。引擎 --exclude-st 加载此表。
    st = {k: v for k, v in m.items() if "ST" in v.upper()}
    with open(os.path.join(REPO, "data/baostock/st_symbols.csv"), "w", newline="", encoding="utf-8") as f:
        w = csv.writer(f); w.writerow(["symbol", "name"])
        for k, v in sorted(st.items()):
            w.writerow([k, v])
    print(f"wrote {len(m)} names -> desktop/ui/src/data/stockNames.json + data/baostock/stock_names.csv; "
          f"{len(st)} ST -> data/baostock/st_symbols.csv")


if __name__ == "__main__":
    main()
