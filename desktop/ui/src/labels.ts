// 桌面端显示文案的单一真相源（枚举/术语映射 + 术语表）。设计 §3。
// 量化标准术语（Sharpe/净值/留档）刻意保留，不在此强译。

export const ACTION_ZH: Record<string, string> = {
  Buy: "买入", Sell: "卖出", Adjust: "调整", Hold: "持有",
};
export const MODE_ZH: Record<string, string> = {
  sim_hard: "模拟·硬", sim_soft: "模拟·软", score_hard: "打分·硬", score_soft: "打分·软",
};
export const SNAPSHOT_FIELD_ZH: Record<string, string> = {
  pos: "仓位", entry_price: "建仓价", bars_held: "持仓根数", nav: "净值",
  peak_nav: "峰值净值", max_drawdown: "最大回撤", turnover: "换手",
  last_increase_date: "末次加仓日", max_price_since_entry: "持仓最高价",
  min_price_since_entry: "持仓最低价", bars_since_exit: "离场后根数",
  last_trip_return: "上轮回合收益", trip: "回合数",
};

export const actionZh = (k: string): string => ACTION_ZH[k] ?? k;
export const modeZh = (k: string): string => MODE_ZH[k] ?? k;
export const snapshotFieldZh = (k: string): string => SNAPSHOT_FIELD_ZH[k] ?? k;

// 一次性术语（散落标签就地引用，保持一致）
export const TERM = {
  bps: "基点", warmup: "热身期", window: "回溯窗", benchmark: "等权基准",
  bars: "根数", missing: "缺失", schtask: "计划任务", runlog: "运行日志",
} as const;

// 模式选择器一次性解释（popover）
export const MODE_GLOSS = "模拟=资金曲线 / 打分=相对排名；硬=取最优 / 软=概率加权";
