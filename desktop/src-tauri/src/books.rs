//! 三账本静态声明。事实源是 deploy/paper_run.cmd——改那边必须同步这里。
//! 参数核对(2026-06-12):scale 60/240,datalen 1023,qfq,warmup 80,window 100(CLI 默认),
//! cost 10bps,b3 top3 soft 周一 commit。
use crate::paths::Workspace;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookKind {
    Single,
    Portfolio,
}

#[derive(Debug, Clone)]
pub struct Book {
    pub id: &'static str,
    pub title: &'static str,
    pub kind: BookKind,
    /// single:标的;portfolio:空。
    pub symbol: &'static str,
    pub tree_rel: &'static str,
    pub state_rel: &'static str,
    pub sig_rel: &'static str,
    /// fetch 周期(分钟,240=日线)。
    pub scale: u32,
}

pub const BOOKS: [Book; 3] = [
    Book {
        id: "b1",
        title: "账本1 · sh600030 60m",
        kind: BookKind::Single,
        symbol: "sh600030",
        tree_rel: "deploy/tree_v4_frozen.yaml",
        state_rel: "paper/paper_sh600030.json",
        sig_rel: "paper/sig_sh600030.json",
        scale: 60,
    },
    Book {
        id: "b2",
        title: "账本2 · sh600036 60m",
        kind: BookKind::Single,
        symbol: "sh600036",
        tree_rel: "deploy/tree_v4_frozen.yaml",
        state_rel: "paper/paper_sh600036.json",
        sig_rel: "paper/sig_sh600036.json",
        scale: 60,
    },
    Book {
        id: "b3",
        title: "账本3 · 组合 top3 日线",
        kind: BookKind::Portfolio,
        symbol: "",
        tree_rel: "deploy/strength_v1_frozen.yaml",
        state_rel: "paper/holdings_top3.json",
        sig_rel: "paper/sig_portfolio.json",
        scale: 240,
    },
];

impl Book {
    pub fn state_path(&self, ws: &Workspace) -> PathBuf {
        ws.root().join(self.state_rel)
    }
    pub fn sig_path(&self, ws: &Workspace) -> PathBuf {
        ws.root().join(self.sig_rel)
    }
    pub fn tree_path(&self, ws: &Workspace) -> PathBuf {
        ws.root().join(self.tree_rel)
    }
    pub fn primary_csv(&self, ws: &Workspace) -> PathBuf {
        ws.paper_dir().join(format!("p_{}.csv", self.symbol))
    }
}

pub fn find_book(id: &str) -> Option<&'static Book> {
    BOOKS.iter().find(|b| b.id == id)
}
