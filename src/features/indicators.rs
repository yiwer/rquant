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
}
