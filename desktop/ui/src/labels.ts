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

export const VERDICT_ZH: Record<string, string> = { PASS: "通过", FALSIFIED: "证伪" };
export const verdictZh = (v: string): string => VERDICT_ZH[v] ?? v;

// 指数显示名（命令实参/CSV 名仍是 csi300/csi500/csi1000，不可改 value，否则找不到 data/baostock/index/<name>.csv）。
export const INDEX_ZH: Record<string, string> = {
  csi300: "沪深300", csi500: "中证500", csi1000: "中证1000",
};
// 找不到映射时回退原值大写（如自定义指数）。
export const indexZh = (k: string): string => INDEX_ZH[k.toLowerCase()] ?? k.toUpperCase();

// regime/分段 标签仅做显示替换（OOS→样本外、train→训练），不改底层 label 值。
export const regimeLabelZh = (label: string): string =>
  label.replace(/OOS/g, "样本外").replace(/train/gi, "训练");

export const SCREEN_TERM = {
  combined: "综合分", quality: "质量分", speculative: "投机分", excess: "超额",
  oos: "样本外超额", breakEven: "盈亏平衡", indexRel: "指数相对", ewRef: "等权基准（不可投）",
} as const;

// 一等术语悬浮解释（antd Tooltip，帮助非专业用户）。
export const TERM_HELP = {
  indexRel: "组合相对可交易指数的超额收益。",
  oos: "训练期之外（样本外）的超额，最可信。",
  breakEven: "策略毛收益被多少单边成本（基点）抹平，越高越抗成本。",
  ewRef: "全市场等权，含微盘不可真实投资，仅作参考基准。",
  combined: "综合分 = 质量分 × (1 + λ·倾斜)。",
} as const;

export const GATE_STATUS_ZH: Record<string, string> = { pass: "通过", fail: "未过", indeterminate: "不定" };
export const FACTOR_TERM = { ic: "IC(信息系数)", icir: "ICIR", rankic: "RankIC", decay: "IC 衰减", layers: "分层收益", mono: "单调性", spread: "多空价差", cert: "认证", alloc: "配置效应", select: "选择效应", drag: "执行拖累", capacity: "容量" } as const;
