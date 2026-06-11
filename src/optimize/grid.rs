use crate::{Error, Result};
use std::collections::BTreeMap;

/// 一个参数轴：name + 取值列表（已展开）。
#[derive(Debug, Clone)]
pub struct GridAxis {
    pub name: String,
    pub values: Vec<f64>,
}

/// 解析 "name=start:stop:step"（闭区间，容差 1e-9）或 "name=v1,v2,…"。
pub fn parse_grid_axis(s: &str) -> Result<GridAxis> {
    let (name, rhs) = s
        .split_once('=')
        .ok_or_else(|| Error::Data(format!("grid '{s}': expected name=values")))?;
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Data(format!("grid '{s}': empty param name")));
    }
    let rhs = rhs.trim();
    if rhs.is_empty() {
        return Err(Error::Data(format!("grid '{s}': empty values")));
    }
    let num = |t: &str| -> Result<f64> {
        t.trim()
            .parse::<f64>()
            .map_err(|e| Error::Data(format!("grid '{s}': bad number '{t}': {e}")))
    };
    let values = if rhs.contains(':') {
        let parts: Vec<&str> = rhs.split(':').collect();
        if parts.len() != 3 {
            return Err(Error::Data(format!("grid '{s}': range needs start:stop:step")));
        }
        let (start, stop, step) = (num(parts[0])?, num(parts[1])?, num(parts[2])?);
        if step <= 0.0 || start > stop {
            return Err(Error::Data(format!("grid '{s}': need step>0 and start<=stop")));
        }
        let mut v = Vec::new();
        let mut x = start;
        while x <= stop + 1e-9 {
            v.push(x);
            x += step;
        }
        v
    } else {
        rhs.split(',').map(num).collect::<Result<Vec<f64>>>()?
    };
    if values.is_empty() {
        return Err(Error::Data(format!("grid '{s}': no values")));
    }
    Ok(GridAxis { name: name.to_string(), values })
}

/// 笛卡尔积（CLI 声明序、末轴变最快、确定性）；重名/空/超上限 → Error。
pub fn expand_grid(axes: &[GridAxis], max_combos: usize) -> Result<Vec<BTreeMap<String, f64>>> {
    if axes.is_empty() {
        return Err(Error::Data("optimize: at least one --grid required".into()));
    }
    for (i, a) in axes.iter().enumerate() {
        if axes[..i].iter().any(|b| b.name == a.name) {
            return Err(Error::Data(format!("optimize: duplicate --grid name '{}'", a.name)));
        }
    }
    let mut total: usize = 1;
    for a in axes {
        total = total
            .checked_mul(a.values.len())
            .ok_or_else(|| Error::Data("optimize: grid size overflow".into()))?;
    }
    if total > max_combos {
        return Err(Error::Data(format!(
            "optimize: {total} combos exceed --max-combos {max_combos}; narrow the grid"
        )));
    }
    let mut combos = Vec::with_capacity(total);
    for mut idx in 0..total {
        let mut m = BTreeMap::new();
        for a in axes.iter().rev() {
            let n = a.values.len();
            m.insert(a.name.clone(), a.values[idx % n]);
            idx /= n;
        }
        combos.push(m);
    }
    Ok(combos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_range_axis_closed_interval() {
        let a = parse_grid_axis("ma_n=10:40:5").unwrap();
        assert_eq!(a.name, "ma_n");
        assert_eq!(a.values, vec![10.0, 15.0, 20.0, 25.0, 30.0, 35.0, 40.0]);
        // 浮点容差闭端：0.1+0.1+0.1 ≈ 0.3 必须包含
        let b = parse_grid_axis("k=0.1:0.3:0.1").unwrap();
        assert_eq!(b.values.len(), 3);
        assert!((b.values[2] - 0.3).abs() < 1e-9);
    }

    #[test]
    fn parses_list_axis() {
        let a = parse_grid_axis("thr=5,15,100").unwrap();
        assert_eq!(a.values, vec![5.0, 15.0, 100.0]);
    }

    #[test]
    fn rejects_malformed() {
        assert!(parse_grid_axis("noequals").is_err());
        assert!(parse_grid_axis("=1,2").is_err());          // 空名
        assert!(parse_grid_axis("a=").is_err());            // 空值
        assert!(parse_grid_axis("a=5:1:1").is_err());       // start>stop
        assert!(parse_grid_axis("a=1:5:0").is_err());       // step=0
        assert!(parse_grid_axis("a=1,x").is_err());         // 非数
    }

    #[test]
    fn expand_cartesian_last_axis_fastest() {
        let axes = vec![
            GridAxis { name: "a".into(), values: vec![1.0, 2.0] },
            GridAxis { name: "b".into(), values: vec![10.0, 20.0] },
        ];
        let combos = expand_grid(&axes, 10).unwrap();
        let get = |i: usize, k: &str| *combos[i].get(k).unwrap();
        assert_eq!(combos.len(), 4);
        assert_eq!((get(0, "a"), get(0, "b")), (1.0, 10.0));
        assert_eq!((get(1, "a"), get(1, "b")), (1.0, 20.0)); // b 变最快
        assert_eq!((get(2, "a"), get(2, "b")), (2.0, 10.0));
    }

    #[test]
    fn rejects_duplicates_and_cap() {
        let axes = vec![
            GridAxis { name: "a".into(), values: vec![1.0] },
            GridAxis { name: "a".into(), values: vec![2.0] },
        ];
        assert!(expand_grid(&axes, 10).is_err()); // 重名
        let big = vec![GridAxis { name: "a".into(), values: (0..600).map(|i| i as f64).collect() }];
        assert!(expand_grid(&big, 500).is_err()); // 超上限
        assert!(expand_grid(&[], 10).is_err());   // 空
    }
}
