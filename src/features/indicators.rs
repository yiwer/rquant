/// 简单移动平均；out\[i\] = mean(s\[i-n+1..=i\])，i < n-1 处为 NaN。
pub fn sma(s: &[f64], n: usize) -> Vec<f64> {
    let mut out = vec![f64::NAN; s.len()];
    if n == 0 {
        return out;
    }
    let mut sum = 0.0;
    for i in 0..s.len() {
        sum += s[i];
        if i >= n {
            sum -= s[i - n];
        }
        if i + 1 >= n {
            out[i] = sum / n as f64;
        }
    }
    out
}

/// 指数移动平均；out\[0\] = s\[0\]，alpha = 2/(n+1)。
pub fn ema(s: &[f64], n: usize) -> Vec<f64> {
    let mut out = vec![f64::NAN; s.len()];
    if s.is_empty() || n == 0 {
        return out;
    }
    let alpha = 2.0 / (n as f64 + 1.0);
    out[0] = s[0];
    for i in 1..s.len() {
        out[i] = alpha * s[i] + (1.0 - alpha) * out[i - 1];
    }
    out
}

fn rsi_from(avg_gain: f64, avg_loss: f64) -> f64 {
    if avg_loss == 0.0 {
        return if avg_gain == 0.0 { 50.0 } else { 100.0 };
    }
    let rs = avg_gain / avg_loss;
    100.0 - 100.0 / (1.0 + rs)
}

/// Wilder RSI；前 n 个位置为 NaN。
pub fn rsi(s: &[f64], n: usize) -> Vec<f64> {
    let len = s.len();
    let mut out = vec![f64::NAN; len];
    if len <= n || n == 0 {
        return out;
    }
    let (mut gain, mut loss) = (0.0, 0.0);
    for i in 1..=n {
        let d = s[i] - s[i - 1];
        if d >= 0.0 {
            gain += d;
        } else {
            loss -= d;
        }
    }
    let mut avg_gain = gain / n as f64;
    let mut avg_loss = loss / n as f64;
    out[n] = rsi_from(avg_gain, avg_loss);
    for i in (n + 1)..len {
        let d = s[i] - s[i - 1];
        let (g, l) = if d >= 0.0 { (d, 0.0) } else { (0.0, -d) };
        avg_gain = (avg_gain * (n as f64 - 1.0) + g) / n as f64;
        avg_loss = (avg_loss * (n as f64 - 1.0) + l) / n as f64;
        out[i] = rsi_from(avg_gain, avg_loss);
    }
    out
}

/// Wilder ATR；前 n-1 个位置为 NaN。high/low/close 等长。
pub fn atr(high: &[f64], low: &[f64], close: &[f64], n: usize) -> Vec<f64> {
    let len = high.len();
    let mut out = vec![f64::NAN; len];
    if len == 0 || n == 0 || low.len() != len || close.len() != len || len < n {
        return out;
    }
    let mut tr = vec![0.0; len];
    tr[0] = high[0] - low[0];
    for i in 1..len {
        let a = high[i] - low[i];
        let b = (high[i] - close[i - 1]).abs();
        let c = (low[i] - close[i - 1]).abs();
        tr[i] = a.max(b).max(c);
    }
    let mut sum = 0.0;
    for v in tr.iter().take(n) {
        sum += *v;
    }
    out[n - 1] = sum / n as f64;
    for i in n..len {
        out[i] = (out[i - 1] * (n as f64 - 1.0) + tr[i]) / n as f64;
    }
    out
}

/// 最近 n 根的线性回归斜率（x = 0..n-1）。不足返回 NaN。
pub fn slope(s: &[f64], n: usize) -> f64 {
    let len = s.len();
    if n < 2 || len < n {
        return f64::NAN;
    }
    let w = &s[len - n..];
    let nf = n as f64;
    let mean_x = (nf - 1.0) / 2.0;
    let mean_y = w.iter().sum::<f64>() / nf;
    let (mut num, mut den) = (0.0, 0.0);
    for (i, &y) in w.iter().enumerate() {
        let dx = i as f64 - mean_x;
        num += dx * (y - mean_y);
        den += dx * dx;
    }
    if den == 0.0 { f64::NAN } else { num / den }
}

/// 最近 n 根最高值。窗口内 NaN 跳过；无有限值（空/全 NaN）返回 NaN（弃权），不返回 −∞。
pub fn highest(s: &[f64], n: usize) -> f64 {
    let len = s.len();
    if len == 0 || n == 0 {
        return f64::NAN;
    }
    let start = len.saturating_sub(n);
    let m = s[start..].iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if m == f64::NEG_INFINITY { f64::NAN } else { m }
}

/// 最近 n 根最低值。窗口内 NaN 跳过；无有限值（空/全 NaN）返回 NaN（弃权），不返回 +∞。
pub fn lowest(s: &[f64], n: usize) -> f64 {
    let len = s.len();
    if len == 0 || n == 0 {
        return f64::NAN;
    }
    let start = len.saturating_sub(n);
    let m = s[start..].iter().copied().fold(f64::INFINITY, f64::min);
    if m == f64::INFINITY { f64::NAN } else { m }
}

/// highest 的滚动序列版：位 j = 窗口 [max(0,j+1−n)..=j] 的 NaN 跳过最大值。
/// 头部为宽容扩张窗（与标量版 len<n 语义一致）；全 NaN 窗 → NaN。
pub fn highest_roll(s: &[f64], n: usize) -> Vec<f64> {
    let mut out = vec![f64::NAN; s.len()];
    if n == 0 {
        return out;
    }
    for j in 0..s.len() {
        let start = (j + 1).saturating_sub(n);
        let m = s[start..=j].iter().copied().fold(f64::NEG_INFINITY, f64::max);
        out[j] = if m == f64::NEG_INFINITY { f64::NAN } else { m };
    }
    out
}

/// lowest 的滚动序列版：位 j = 窗口 [max(0,j+1−n)..=j] 的 NaN 跳过最小值。
/// 头部为宽容扩张窗（与标量版 len<n 语义一致）；全 NaN 窗 → NaN。
pub fn lowest_roll(s: &[f64], n: usize) -> Vec<f64> {
    let mut out = vec![f64::NAN; s.len()];
    if n == 0 {
        return out;
    }
    for j in 0..s.len() {
        let start = (j + 1).saturating_sub(n);
        let m = s[start..=j].iter().copied().fold(f64::INFINITY, f64::min);
        out[j] = if m == f64::INFINITY { f64::NAN } else { m };
    }
    out
}

/// std 的滚动序列版：位 j+1<n → NaN（严格头，镜像标量版 len<n → NaN）；窗含 NaN → NaN 传播。
pub fn std_roll(s: &[f64], n: usize) -> Vec<f64> {
    let mut out = vec![f64::NAN; s.len()];
    if n == 0 {
        return out;
    }
    for j in 0..s.len() {
        if j + 1 < n {
            continue;
        }
        let w = &s[j + 1 - n..=j];
        let mean = w.iter().sum::<f64>() / n as f64;
        let var = w.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n as f64;
        out[j] = var.sqrt();
    }
    out
}

/// slope 的滚动序列版：OLS 斜率逐位；n<2 或 j+1<n → NaN（严格头）；
/// 窗含 NaN → NaN 传播（不跳过，与 std_roll 一致，与 highest/lowest_roll 的跳过语义相反）。
pub fn slope_roll(s: &[f64], n: usize) -> Vec<f64> {
    let mut out = vec![f64::NAN; s.len()];
    if n < 2 {
        return out;
    }
    for j in 0..s.len() {
        if j + 1 < n {
            continue;
        }
        let w = &s[j + 1 - n..=j];
        let nf = n as f64;
        let mean_x = (nf - 1.0) / 2.0;
        let mean_y = w.iter().sum::<f64>() / nf;
        let (mut num, mut den) = (0.0, 0.0);
        for (i, &y) in w.iter().enumerate() {
            let dx = i as f64 - mean_x;
            num += dx * (y - mean_y);
            den += dx * dx;
        }
        out[j] = if den == 0.0 { f64::NAN } else { num / den };
    }
    out
}

/// a 上穿 b：上一根 a<=b 且本根 a>b。
pub fn crossover(a: &[f64], b: &[f64]) -> bool {
    let (la, lb) = (a.len(), b.len());
    if la < 2 || lb < 2 {
        return false;
    }
    a[la - 2] <= b[lb - 2] && a[la - 1] > b[lb - 1]
}

/// a 下穿 b：上一根 a>=b 且本根 a<b。
pub fn crossunder(a: &[f64], b: &[f64]) -> bool {
    let (la, lb) = (a.len(), b.len());
    if la < 2 || lb < 2 {
        return false;
    }
    a[la - 2] >= b[lb - 2] && a[la - 1] < b[lb - 1]
}

/// 线性加权移动平均（权重 1..n，最新最重）；前 n-1 位为 NaN。
pub fn wma(s: &[f64], n: usize) -> Vec<f64> {
    let len = s.len();
    let mut out = vec![f64::NAN; len];
    if n == 0 || len < n {
        return out;
    }
    let denom = (n * (n + 1) / 2) as f64;
    for (idx, slot) in out.iter_mut().enumerate().skip(n - 1) {
        let mut acc = 0.0;
        let start = idx + 1 - n;
        for k in 0..n {
            acc += s[start + k] * (k + 1) as f64;
        }
        *slot = acc / denom;
    }
    out
}

/// 最近 n 根的总体标准差（÷n）；不足返回 NaN。
pub fn std(s: &[f64], n: usize) -> f64 {
    let len = s.len();
    if n == 0 || len < n {
        return f64::NAN;
    }
    let w = &s[len - n..];
    let mean = w.iter().sum::<f64>() / n as f64;
    let var = w.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n as f64;
    var.sqrt()
}

/// MACD 快线：ema(fast) - ema(slow) 逐点（ema 等长，下标对齐）。
pub fn macd_line(s: &[f64], fast: usize, slow: usize) -> Vec<f64> {
    let f = ema(s, fast);
    let g = ema(s, slow);
    f.iter().zip(g.iter()).map(|(a, b)| a - b).collect()
}

/// MACD 信号线：ema(macd_line, sig)。
pub fn macd_signal(s: &[f64], fast: usize, slow: usize, sig: usize) -> Vec<f64> {
    ema(&macd_line(s, fast, slow), sig)
}

/// MACD 柱：macd_line - macd_signal 逐点。
pub fn macd_hist(s: &[f64], fast: usize, slow: usize, sig: usize) -> Vec<f64> {
    let line = macd_line(s, fast, slow);
    let signal = macd_signal(s, fast, slow, sig);
    line.iter().zip(signal.iter()).map(|(a, b)| a - b).collect()
}

/// percentrank 滚动序列版：位 j = 窗口（含当前，长 n）内严格小于 s[j] 的个数 / (n−1) ∈ [0,1]。
/// n<2 或 j+1<n → NaN（严格头，同 std_roll）；窗含 NaN → NaN 传播。
pub fn percentrank_roll(s: &[f64], n: usize) -> Vec<f64> {
    let mut out = vec![f64::NAN; s.len()];
    if n < 2 {
        return out;
    }
    for j in 0..s.len() {
        if j + 1 < n {
            continue;
        }
        let w = &s[j + 1 - n..=j];
        // NaN 传播：窗口含任何 NaN → 输出 NaN
        if w.iter().any(|x| x.is_nan()) {
            continue; // out[j] stays NaN
        }
        let cur = w[n - 1]; // w 末位即 s[j]
        let count = w.iter().filter(|&&x| x < cur).count();
        out[j] = count as f64 / (n - 1) as f64;
    }
    out
}

/// corr 滚动序列版：位 j = 窗口（长 n）内 a/b 两序列的 Pearson 相关系数。
/// a/b 已等长（对齐在 eval 臂完成）。n<2 或 j+1<n → NaN（严格头）；
/// 窗含 NaN → NaN；任一侧零方差 → NaN。
pub fn corr_roll(a: &[f64], b: &[f64], n: usize) -> Vec<f64> {
    let len = a.len().min(b.len());
    let mut out = vec![f64::NAN; len];
    if n < 2 {
        return out;
    }
    for j in 0..len {
        if j + 1 < n {
            continue;
        }
        let wa = &a[j + 1 - n..=j];
        let wb = &b[j + 1 - n..=j];
        // NaN 传播：任一序列窗口含 NaN → NaN
        if wa.iter().any(|x| x.is_nan()) || wb.iter().any(|x| x.is_nan()) {
            continue;
        }
        let nf = n as f64;
        let mean_a = wa.iter().sum::<f64>() / nf;
        let mean_b = wb.iter().sum::<f64>() / nf;
        let (mut cov, mut var_a, mut var_b) = (0.0_f64, 0.0_f64, 0.0_f64);
        for (&x, &y) in wa.iter().zip(wb.iter()) {
            let da = x - mean_a;
            let db = y - mean_b;
            cov += da * db;
            var_a += da * da;
            var_b += db * db;
        }
        let denom = var_a.sqrt() * var_b.sqrt();
        // 任一侧零方差（常数序列）→ NaN 弃权
        out[j] = if denom == 0.0 { f64::NAN } else { (cov / denom).clamp(-1.0, 1.0) };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn sma_basic() {
        let out = sma(&[1.0, 2.0, 3.0, 4.0, 5.0], 3);
        assert!(out[0].is_nan());
        assert!(out[1].is_nan());
        assert_relative_eq!(out[2], 2.0);
        assert_relative_eq!(out[3], 3.0);
        assert_relative_eq!(out[4], 4.0);
    }

    #[test]
    fn ema_constant_series_is_constant() {
        let out = ema(&[5.0, 5.0, 5.0, 5.0], 3);
        assert_relative_eq!(out[0], 5.0);
        assert_relative_eq!(out[3], 5.0);
    }

    #[test]
    fn rsi_increasing_is_100_decreasing_is_0() {
        let up: Vec<f64> = (0..30).map(|i| i as f64).collect();
        let down: Vec<f64> = (0..30).map(|i| (30 - i) as f64).collect();
        assert_relative_eq!(*rsi(&up, 14).last().unwrap(), 100.0);
        assert_relative_eq!(*rsi(&down, 14).last().unwrap(), 0.0);
    }

    #[test]
    fn atr_constant_range() {
        let high = vec![11.0; 10];
        let low = vec![9.0; 10];
        let close = vec![10.0; 10];
        let out = atr(&high, &low, &close, 3);
        assert_relative_eq!(*out.last().unwrap(), 2.0);
    }

    #[test]
    fn slope_of_linear_series() {
        assert_relative_eq!(slope(&[1.0, 2.0, 3.0, 4.0, 5.0], 5), 1.0);
    }

    #[test]
    fn highest_lowest_last_n() {
        let s = [3.0, 1.0, 4.0, 1.0, 5.0, 9.0, 2.0];
        assert_relative_eq!(highest(&s, 3), 9.0);
        assert_relative_eq!(lowest(&s, 3), 2.0);
    }

    #[test]
    fn highest_lowest_all_nan_abstains() {
        // 全 NaN 窗口必须返回 NaN（弃权），不能返回 ±∞ 让比较意外触发
        let s = [f64::NAN, f64::NAN, f64::NAN];
        assert!(highest(&s, 3).is_nan());
        assert!(lowest(&s, 3).is_nan());
    }

    #[test]
    fn highest_lowest_skip_nan_keep_finite() {
        // 窗口内有有限值时跳过 NaN（IEEE max/min 语义），取有限值
        let s = [f64::NAN, 5.0, f64::NAN];
        assert_relative_eq!(highest(&s, 3), 5.0);
        assert_relative_eq!(lowest(&s, 3), 5.0);
    }

    #[test]
    fn cross_detection() {
        assert!(crossover(&[1.0, 3.0], &[2.0, 2.0]));
        assert!(!crossover(&[3.0, 4.0], &[2.0, 2.0]));
        assert!(crossunder(&[3.0, 1.0], &[2.0, 2.0]));
    }

    #[test]
    fn wma_known_value() {
        let out = wma(&[1.0, 2.0, 3.0], 3);
        assert!(out[0].is_nan());
        assert!(out[1].is_nan());
        assert_relative_eq!(out[2], 14.0 / 6.0); // (1*1 + 2*2 + 3*3) / (1+2+3)
    }

    #[test]
    fn std_population() {
        // [1,2,3,4,5]: mean 3, var=(4+1+0+1+4)/5=2, std=sqrt(2)
        assert_relative_eq!(std(&[1.0, 2.0, 3.0, 4.0, 5.0], 5), 2.0_f64.sqrt());
    }

    #[test]
    fn macd_zero_on_constant_series() {
        let s = vec![5.0; 30];
        assert!(macd_line(&s, 12, 26).last().unwrap().abs() < 1e-9);
        assert!(macd_signal(&s, 12, 26, 9).last().unwrap().abs() < 1e-9);
        assert!(macd_hist(&s, 12, 26, 9).last().unwrap().abs() < 1e-9);
    }

    /// 滚动版末位 == 旧标量版（含 len<n 的宽容/严格差异），NaN 位用 bits 比较。
    #[test]
    fn roll_last_equals_scalar_form() {
        let s = [3.0, f64::NAN, 5.0, 1.0, 4.0, 2.0];
        for n in [1usize, 3, 6, 99] {
            for len in 1..=s.len() {
                let w = &s[..len];
                let pairs: [(f64, f64); 4] = [
                    (*highest_roll(w, n).last().unwrap(), highest(w, n)),
                    (*lowest_roll(w, n).last().unwrap(), lowest(w, n)),
                    (*std_roll(w, n).last().unwrap(), std(w, n)),
                    (*slope_roll(w, n).last().unwrap(), slope(w, n)),
                ];
                for (i, (r, sc)) in pairs.iter().enumerate() {
                    assert!(
                        r.to_bits() == sc.to_bits() || (r.is_nan() && sc.is_nan()),
                        "fn#{i} n={n} len={len}: roll last {r} != scalar {sc}"
                    );
                }
            }
        }
    }

    #[test]
    fn highest_roll_head_is_expanding_window() {
        // 宽容头部：j<n-1 时窗口为 [0..=j]（运行最值），与标量版短序列语义一致
        let s = [2.0, 5.0, 1.0, 4.0];
        assert_eq!(highest_roll(&s, 3), vec![2.0, 5.0, 5.0, 5.0]);
        assert_eq!(lowest_roll(&s, 3), vec![2.0, 2.0, 1.0, 1.0]);
    }

    #[test]
    fn std_slope_roll_head_is_nan_prefix() {
        // 严格头部：j+1<n → NaN（镜像标量版 len<n → NaN）
        let s = [1.0, 2.0, 3.0, 4.0];
        let sd = std_roll(&s, 3);
        assert!(sd[0].is_nan() && sd[1].is_nan());
        assert!((sd[2] - std(&s[..3], 3)).abs() < 1e-12);
        let sl = slope_roll(&s, 3);
        assert!(sl[0].is_nan() && sl[1].is_nan());
        assert!((sl[3] - 1.0).abs() < 1e-12); // 等差数列斜率=1
    }

    /// percentrank_roll：手算小窗用例 + 严格头 NaN + NaN 传播
    ///
    /// 演算（s = [1.0, 3.0, 2.0], n = 3）：
    ///   j=0: j+1<3 → NaN（严格头）
    ///   j=1: j+1=2<3 → NaN（严格头）
    ///   j=2: 窗口 [1,3,2]，cur=2，严格小于 2 的：{1} → count=1，result=1/(3-1)=0.5
    ///
    /// 演算（s = [1.0, 2.0, 3.0, 4.0, 5.0], n = 3）：
    ///   j=0,1: NaN（严格头）
    ///   j=2: 窗口 [1,2,3]，cur=3，严格小于 3：{1,2} → 2/2=1.0
    ///   j=3: 窗口 [2,3,4]，cur=4，严格小于 4：{2,3} → 2/2=1.0
    ///   j=4: 窗口 [3,4,5]，cur=5，严格小于 5：{3,4} → 2/2=1.0
    ///
    /// 最小值测试（s = [3.0, 1.0, 2.0], n = 3）：
    ///   j=2: 窗口 [3,1,2]，cur=2，严格小于 2：{1} → 1/2=0.5
    ///   注：若 cur 是窗口最小值（s=[3,2,1],n=3,j=2），cur=1，严格小于 1：{} → 0/2=0.0
    #[test]
    fn percentrank_roll_hand_calculated() {
        // 基本三元素窗
        let s = [1.0_f64, 3.0, 2.0];
        let out = percentrank_roll(&s, 3);
        assert!(out[0].is_nan(), "j=0 严格头应为 NaN");
        assert!(out[1].is_nan(), "j=1 严格头应为 NaN");
        assert!((out[2] - 0.5).abs() < 1e-12, "j=2: 1/2=0.5, got {}", out[2]);

        // 递增序列 n=3：后三位均最大值 → percentrank=1.0
        let s2 = [1.0_f64, 2.0, 3.0, 4.0, 5.0];
        let out2 = percentrank_roll(&s2, 3);
        assert!(out2[0].is_nan() && out2[1].is_nan());
        assert!((out2[2] - 1.0).abs() < 1e-12);
        assert!((out2[3] - 1.0).abs() < 1e-12);
        assert!((out2[4] - 1.0).abs() < 1e-12);

        // 窗口最小值 → percentrank=0.0
        // s=[3,2,1],n=3,j=2: cur=1, 严格小于 1 的：{} → 0/(3-1)=0.0
        let s3 = [3.0_f64, 2.0, 1.0];
        let out3 = percentrank_roll(&s3, 3);
        assert!((out3[2] - 0.0).abs() < 1e-12, "最小值 percentrank=0, got {}", out3[2]);

        // n<2 → 全 NaN
        let out_n1 = percentrank_roll(&s2, 1);
        assert!(out_n1.iter().all(|x| x.is_nan()));
        let out_n0 = percentrank_roll(&s2, 0);
        assert!(out_n0.iter().all(|x| x.is_nan()));
    }

    #[test]
    fn percentrank_roll_nan_propagation() {
        // 窗口含 NaN → 该位输出 NaN
        let s = [1.0_f64, f64::NAN, 3.0];
        let out = percentrank_roll(&s, 3);
        // j=2: 窗口 [1, NaN, 3]，含 NaN → NaN
        assert!(out[2].is_nan(), "窗口含 NaN 应传播 NaN");
        // 若窗口不含 NaN 则正常计算
        let s2 = [f64::NAN, 2.0, 3.0, 4.0];
        let out2 = percentrank_roll(&s2, 2);
        // j=0: NaN 严格头; j=1: 窗 [NaN,2]，含 NaN → NaN; j=2: 窗 [2,3] → 1/1=1.0; j=3: 窗 [3,4] → 1/1=1.0
        assert!(out2[1].is_nan(), "j=1 窗口含 NaN");
        assert!((out2[2] - 1.0).abs() < 1e-12, "j=2 正常: {}", out2[2]);
        assert!((out2[3] - 1.0).abs() < 1e-12, "j=3 正常: {}", out2[3]);
    }

    /// corr_roll：手算完全正/负相关 ±1 + 零方差 NaN + 严格头 NaN + NaN 传播
    ///
    /// 演算（完全正相关）：
    ///   a = [1,2,3], b = [2,4,6]（b = 2a），n=3
    ///   j=2: mean_a=2, mean_b=4
    ///   cov = (1-2)(2-4)+(2-2)(4-4)+(3-2)(6-4) = (-1)(-2)+0+1*2 = 2+0+2=4
    ///   var_a = 1+0+1=2, var_b = 4+0+4=8
    ///   denom = sqrt(2)*sqrt(8) = sqrt(16)=4, corr=4/4=1.0
    ///
    /// 演算（完全负相关）：
    ///   a = [1,2,3], b = [6,4,2]（b = 8-2a），n=3
    ///   j=2: mean_a=2, mean_b=4
    ///   cov = (-1)(2)+0+(1)(-2)=-2+0-2=-4
    ///   var_a=2, var_b=8, denom=4, corr=-4/4=-1.0
    #[test]
    fn corr_roll_hand_calculated() {
        // 完全正相关
        let a = [1.0_f64, 2.0, 3.0];
        let b = [2.0_f64, 4.0, 6.0];
        let out = corr_roll(&a, &b, 3);
        assert!(out[0].is_nan() && out[1].is_nan(), "严格头应为 NaN");
        assert!((out[2] - 1.0).abs() < 1e-9, "完全正相关应为 1.0, got {}", out[2]);

        // 完全负相关
        let c = [6.0_f64, 4.0, 2.0];
        let out2 = corr_roll(&a, &c, 3);
        assert!((out2[2] - (-1.0)).abs() < 1e-9, "完全负相关应为 -1.0, got {}", out2[2]);

        // 不相关（近似）：a=[1,2,3,4,5], b=[1,3,2,5,4]，n=3 末窗 [3,4,5] vs [2,5,4]
        // 手算 j=4: mean_a=4,mean_b=11/3; da=[-1,0,1], db=[2-11/3,5-11/3,4-11/3]=[-5/3,4/3,1/3]
        // cov=(-1)(-5/3)+0+(1)(1/3)=5/3+1/3=2, var_a=2, var_b=25/9+16/9+1/9=42/9=14/3
        // denom=sqrt(2)*sqrt(14/3)=sqrt(28/3), corr=2/sqrt(28/3)=2*sqrt(3)/sqrt(28)≈0.655
        let a2 = [1.0_f64, 2.0, 3.0, 4.0, 5.0];
        let b2 = [1.0_f64, 3.0, 2.0, 5.0, 4.0];
        let out3 = corr_roll(&a2, &b2, 3);
        assert!(out3[4].is_finite(), "相关系数应有有限值");
        assert!(out3[4] > -1.0 && out3[4] < 1.0);

        // n<2 → 全 NaN
        let out_n1 = corr_roll(&a, &b, 1);
        assert!(out_n1.iter().all(|x| x.is_nan()));
    }

    #[test]
    fn corr_roll_zero_variance_is_nan() {
        // 零方差（常数序列）→ NaN 弃权
        let a = [2.0_f64, 2.0, 2.0];
        let b = [1.0_f64, 2.0, 3.0];
        let out = corr_roll(&a, &b, 3);
        assert!(out[2].is_nan(), "零方差 a 应为 NaN");

        let out2 = corr_roll(&b, &a, 3);
        assert!(out2[2].is_nan(), "零方差 b 应为 NaN");

        // 双侧零方差
        let out3 = corr_roll(&a, &a, 3);
        assert!(out3[2].is_nan(), "双侧零方差应为 NaN");
    }

    #[test]
    fn corr_roll_nan_propagation() {
        let a = [1.0_f64, f64::NAN, 3.0];
        let b = [1.0_f64, 2.0, 3.0];
        let out = corr_roll(&a, &b, 3);
        assert!(out[2].is_nan(), "a 窗口含 NaN 应传播 NaN");

        let a2 = [1.0_f64, 2.0, 3.0];
        let b2 = [1.0_f64, f64::NAN, 3.0];
        let out2 = corr_roll(&a2, &b2, 3);
        assert!(out2[2].is_nan(), "b 窗口含 NaN 应传播 NaN");
    }
}
