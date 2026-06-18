// 把常见后端(Rust)报错原文映射为友好中文；原文保留于 detail 供量化用户排查。设计 §5.1。
const RULES: ReadonlyArray<readonly [RegExp, string]> = [
  [/parse|yaml|tree.*error|decision tree|expected .*found/i, "策略树解析失败"],
  [/no such file|not found|cannot find|读取失败/i, "文件未找到或无法读取"],
  [/python|iterate\.py|no module|modulenotfound/i, "未找到 Python 或 harness 依赖（确认已装 Python 与依赖）"],
  [/index|csi\d+|指数数据/i, "缺少基准指数数据（运行 scripts/fetch_index.py）"],
  [/universe|无可选标的|empty/i, "该日无可选标的（检查成分/数据范围）"],
  [/fetch|tencent|sina|http|network|request error|timeout/i, "数据拉取失败（网络或数据源）"],
  [/csv|bad number|column|row too short|header/i, "数据文件格式错误"],
];

export function friendlyError(raw: string): { title: string; detail: string } {
  for (const [re, msg] of RULES) {
    if (re.test(raw)) return { title: msg, detail: raw };
  }
  return { title: "操作失败", detail: raw };
}
