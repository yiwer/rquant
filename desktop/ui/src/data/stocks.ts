import names from "./stockNames.json";

// symbol(sh600000) → 中文名称。静态映射(scripts/build_stock_names.py 经 akshare 生成，5500+ A股)。
export const stockNames = names as Record<string, string>;

/** 名称（未知返回 "—"）。 */
export const stockName = (s: string): string => stockNames[s] ?? "—";

/** 名称（未知回退到代码），用于紧凑展示。 */
export const stockLabel = (s: string): string => stockNames[s] ?? s;

/** 是否 ST/*ST 高风险股（按名称判定）。未知名称按非 ST 处理。 */
export const isST = (s: string): boolean => (stockNames[s] ?? "").toUpperCase().includes("ST");
