use crate::{Error, Result};
use std::path::{Path, PathBuf};

/// universe 一行：标的 + 其 primary/context CSV 路径（context 缺省=primary）。
pub struct UniverseEntry {
    pub symbol: String,
    pub primary: PathBuf,
    pub context: PathBuf,
}

/// 读 universe CSV（表头 symbol,primary[,context]）；symbol 非空且唯一；按 symbol 字典序返回。
pub fn read_universe_csv(path: &Path) -> Result<Vec<UniverseEntry>> {
    let mut rdr = csv::Reader::from_path(path)?;
    let headers = rdr.headers()?.clone();
    if headers.len() < 2 || &headers[0] != "symbol" || &headers[1] != "primary" {
        return Err(Error::Data("universe csv must start with columns: symbol,primary[,context]".into()));
    }
    let has_ctx = headers.len() >= 3 && &headers[2] == "context";
    let mut out: Vec<UniverseEntry> = Vec::new();
    for rec in rdr.records() {
        let rec = rec?;
        let symbol = rec[0].trim().to_string();
        if symbol.is_empty() {
            return Err(Error::Data("universe: empty symbol".into()));
        }
        if out.iter().any(|e| e.symbol == symbol) {
            return Err(Error::Data(format!("universe: duplicate symbol '{symbol}'")));
        }
        let primary = PathBuf::from(rec[1].trim());
        let context = if has_ctx && !rec[2].trim().is_empty() {
            PathBuf::from(rec[2].trim())
        } else {
            primary.clone()
        };
        out.push(UniverseEntry { symbol, primary, context });
    }
    out.sort_by(|a, b| a.symbol.cmp(&b.symbol));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_tmp(content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn reads_two_and_three_column_and_sorts() {
        let f = write_tmp("symbol,primary,context\nsz000001,b.csv,bc.csv\nsh600000,a.csv,\n");
        let u = read_universe_csv(f.path()).unwrap();
        assert_eq!(u.len(), 2);
        assert_eq!(u[0].symbol, "sh600000"); // 字典序
        assert_eq!(u[0].context, u[0].primary); // 空 context 回退 primary
        assert_eq!(u[1].context.to_str().unwrap(), "bc.csv");
        // 两列表头也合法
        let f2 = write_tmp("symbol,primary\nsh600000,a.csv\n");
        assert_eq!(read_universe_csv(f2.path()).unwrap()[0].context.to_str().unwrap(), "a.csv");
    }

    #[test]
    fn rejects_duplicate_and_empty_symbol() {
        assert!(read_universe_csv(write_tmp("symbol,primary\ns1,a.csv\ns1,b.csv\n").path()).is_err());
        assert!(read_universe_csv(write_tmp("symbol,primary\n,a.csv\n").path()).is_err());
    }
}
