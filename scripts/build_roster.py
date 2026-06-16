"""构建全市场 roster（含退市股）→ data/universe_full.csv。
每行 symbol,primary,context,fundamentals；context 留空(=primary)；fundamentals 已存在则填。
用法: python scripts/build_roster.py [--out data/universe_full.csv] [--data-dir data] [--fund-dir data/fundamentals]"""
import argparse, os, sys
import akshare as ak

def to_symbol(code):
    code = str(code).zfill(6)
    if code[:2] in ("60", "68") or code[0] == "9":
        return "sh" + code
    if code[:2] in ("00", "30") or code[0] == "2":
        return "sz" + code
    return None

def collect_codes():
    syms = set()
    try:
        df = ak.stock_info_a_code_name()  # columns: code, name
        for c in df["code"]:
            s = to_symbol(c)
            if s: syms.add(s)
        print(f"  in-listed: {len(syms)}", file=sys.stderr)
    except Exception as e:
        print(f"WARN in-listed list failed: {e}", file=sys.stderr)
    for fn in ("stock_info_sh_delist", "stock_info_sz_delist"):
        try:
            d = getattr(ak, fn)()
            col = next((c for c in d.columns if "代码" in str(c)), None)
            if col is None:
                print(f"WARN {fn}: no code column in {list(d.columns)}", file=sys.stderr); continue
            cnt = 0
            for c in d[col]:
                s = to_symbol(c)
                if s: syms.add(s); cnt += 1
            print(f"  {fn}: +{cnt}", file=sys.stderr)
        except Exception as e:
            print(f"WARN {fn} failed: {e}", file=sys.stderr)
    return sorted(syms)

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="data/universe_full.csv")
    ap.add_argument("--data-dir", default="data")
    ap.add_argument("--fund-dir", default="data/fundamentals")
    args = ap.parse_args()
    syms = collect_codes()
    with open(args.out, "w", encoding="utf-8", newline="") as f:
        f.write("symbol,primary,context,fundamentals\n")
        for s in syms:
            fund = f"{args.fund_dir}/{s}.csv"
            fund_col = fund if os.path.exists(fund) else ""
            f.write(f"{s},{args.data_dir}/{s}.csv,,{fund_col}\n")
    print(f"wrote {len(syms)} symbols to {args.out}")

if __name__ == "__main__":
    main()
