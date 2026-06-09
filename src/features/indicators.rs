/// 简单移动平均；out[i] = mean(s[i-n+1..=i])，i < n-1 处为 NaN。
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

/// 指数移动平均；out[0] = s[0]，alpha = 2/(n+1)。
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

/// 最近 n 根最高值。
pub fn highest(s: &[f64], n: usize) -> f64 {
    let len = s.len();
    if len == 0 || n == 0 {
        return f64::NAN;
    }
    let start = len.saturating_sub(n);
    s[start..].iter().copied().fold(f64::NEG_INFINITY, f64::max)
}

/// 最近 n 根最低值。
pub fn lowest(s: &[f64], n: usize) -> f64 {
    let len = s.len();
    if len == 0 || n == 0 {
        return f64::NAN;
    }
    let start = len.saturating_sub(n);
    s[start..].iter().copied().fold(f64::INFINITY, f64::min)
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
}
