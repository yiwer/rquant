use crate::{Error, Result};
use chrono::NaiveDateTime;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct NewsRecord {
    pub time: NaiveDateTime,
    pub score: f64,
    pub headline: String,
}

/// 决策时点可见的最近若干条新闻（time <= t）。仅供 LLM 渲染读取。
#[derive(Debug, Clone)]
pub struct NewsView {
    pub recent: Vec<NewsRecord>,
}

#[derive(serde::Deserialize)]
struct Row {
    time: String,
    score: f64,
    headline: String,
}

/// 读取新闻 CSV（表头 time,score,headline）为按时间升序的记录。
/// 允许同一时间多条，但不允许时间回退。
pub fn read_news_csv(path: &Path) -> Result<Vec<NewsRecord>> {
    let mut rdr = csv::Reader::from_path(path)?;
    let mut out: Vec<NewsRecord> = Vec::new();
    for rec in rdr.deserialize() {
        let row: Row = rec?;
        let time = NaiveDateTime::parse_from_str(&row.time, "%Y-%m-%d %H:%M:%S")
            .map_err(|e| Error::Data(format!("bad news time '{}': {e}", row.time)))?;
        if let Some(prev) = out.last()
            && time < prev.time
        {
            return Err(Error::Data(format!("news time out of order at {time}")));
        }
        out.push(NewsRecord { time, score: row.score, headline: row.headline });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        write!(f, "{content}").unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn reads_news_csv() {
        let f = tmp("time,score,headline\n2024-01-02 09:30:00,0.8,good A\n2024-01-02 10:00:00,-0.5,bad B\n");
        let n = read_news_csv(f.path()).unwrap();
        assert_eq!(n.len(), 2);
        assert_eq!(n[0].score, 0.8);
        assert_eq!(n[1].headline, "bad B");
    }

    #[test]
    fn rejects_out_of_order() {
        let f = tmp("time,score,headline\n2024-01-02 10:00:00,0.1,a\n2024-01-02 09:00:00,0.1,b\n");
        assert!(read_news_csv(f.path()).is_err());
    }
}
