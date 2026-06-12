//! 账本卡片/diff/快照读取——全只读,引擎零改动(spec §5.1)。
//! 设计要点:state 与 sig 各有自己的时间戳,可能不一致(dry 残留),如实分开返回。

use crate::books::{Book, BookKind};
use crate::dto::{BookCardDto, DiffRowDto, SignalBriefDto, SnapshotDto};
use crate::paths::Workspace;
use rquant::backtest::sim::AccountSnapshot;
use rquant::signal::{read_holdings_state, read_paper_state, PortfolioSignal, SingleSignal};

fn iso(t: &chrono::NaiveDateTime) -> String {
    t.format("%Y-%m-%dT%H:%M:%S").to_string()
}

pub fn snapshot_to_dto(s: &AccountSnapshot) -> SnapshotDto {
    SnapshotDto {
        pos: s.pos,
        entry_price: s.entry_price,
        bars_held: s.bars_held as u64,
        nav: s.nav,
        peak_nav: s.peak_nav,
        max_drawdown: s.max_drawdown,
        turnover: s.turnover,
        last_increase_date: s.last_increase_date.map(|d| d.to_string()),
        max_price_since_entry: s.max_price_since_entry,
        min_price_since_entry: s.min_price_since_entry,
        bars_since_exit: s.bars_since_exit,
        last_trip_return: s.last_trip_return,
        trip: s.trip.as_ref().map(|t| serde_json::to_value(t).unwrap_or(serde_json::Value::Null)),
    }
}

fn read_single_sig(path: &std::path::Path) -> Option<SignalBriefDto> {
    let txt = std::fs::read_to_string(path).ok()?;
    let sig: SingleSignal = serde_json::from_str(&txt).ok()?;
    Some(SignalBriefDto {
        t: iso(&sig.t),
        target: Some(sig.target),
        current_pos: Some(sig.current_pos),
        delta: Some(sig.delta),
        reason: Some(sig.reason),
        leaf: sig.leaf,
        bars_replayed: Some(sig.paper.bars_replayed as u64),
        targets: None,
        n_fresh: None,
    })
}

fn read_portfolio_sig(path: &std::path::Path) -> Option<(SignalBriefDto, Vec<DiffRowDto>)> {
    let txt = std::fs::read_to_string(path).ok()?;
    let sig: PortfolioSignal = serde_json::from_str(&txt).ok()?;
    let brief = SignalBriefDto {
        t: iso(&sig.t),
        target: None,
        current_pos: None,
        delta: None,
        reason: None,
        leaf: None,
        bars_replayed: None,
        targets: Some(sig.targets.clone()),
        n_fresh: Some(sig.n_fresh as u64),
    };
    let rows = sig
        .trades
        .iter()
        .map(|tr| DiffRowDto {
            symbol: tr.symbol.clone(),
            action: format!("{:?}", tr.action),
            from_w: tr.from_w,
            to_w: tr.to_w,
        })
        .collect();
    Some((brief, rows))
}

/// 树名取自冻结树文件 meta(勿硬编码);树文件本身坏了也归为 corrupt 卡。
// TODO(M2): cache tree per path(b1/b2 共享同一树文件,每次刷新重复 load ~ms 级,M1 可忽略)
fn tree_name(ws: &Workspace, book: &Book) -> Result<String, String> {
    rquant::tree::loader::load_tree_file(&book.tree_path(ws))
        .map(|t| t.meta.name)
        .map_err(|e| e.to_string())
}

pub fn read_book_card(ws: &Workspace, book: &Book) -> BookCardDto {
    let mut card = BookCardDto {
        book: book.id.to_string(),
        title: book.title.to_string(),
        kind: match book.kind {
            BookKind::Single => "single",
            BookKind::Portfolio => "portfolio",
        }
        .to_string(),
        status: "empty".to_string(),
        advice: None,
        nav: None,
        total_return: None,
        max_drawdown: None,
        pos: None,
        state_time: None,
        holdings: None,
        last_signal: None,
    };

    match book.kind {
        BookKind::Single => {
            card.last_signal = read_single_sig(&book.sig_path(ws));
            match tree_name(ws, book) {
                Ok(name) => match read_paper_state(&book.state_path(ws), &name) {
                    Ok(Some(st)) => {
                        card.status = "ok".into();
                        card.nav = Some(st.account.nav);
                        card.total_return = Some(st.account.nav - 1.0);
                        card.max_drawdown = Some(st.account.max_drawdown);
                        card.pos = Some(st.account.pos);
                        card.state_time = st.last_time.as_ref().map(iso);
                    }
                    Ok(None) => {
                        card.advice = Some(
                            "state 未建账:等待 15:35 schtask 首跑,或手动触发 run(收盘后)".into(),
                        );
                    }
                    Err(e) => {
                        let e_str = e.to_string();
                        card.status = "corrupt".into();
                        card.advice = Some(
                            crate::error::ErrorDto::from_anyhow(&anyhow::anyhow!(e_str))
                                .advice
                                .unwrap_or_else(|| {
                                    "state 异常:查看消息并考虑删除重建(重放幂等)".into()
                                }),
                        );
                    }
                },
                Err(e) => {
                    card.status = "corrupt".into();
                    card.advice = Some(
                        crate::error::ErrorDto::from_anyhow(&anyhow::anyhow!(e))
                            .advice
                            .unwrap_or_else(|| {
                                "state 异常:查看消息并考虑删除重建(重放幂等)".into()
                            }),
                    );
                }
            }
        }
        BookKind::Portfolio => {
            if let Some((brief, _)) = read_portfolio_sig(&book.sig_path(ws)) {
                card.last_signal = Some(brief);
            }
            match tree_name(ws, book) {
                Ok(name) => match read_holdings_state(&book.state_path(ws), &name) {
                    Ok(Some(st)) => {
                        card.status = "ok".into();
                        card.holdings =
                            Some(st.holdings.iter().map(|(s, w)| (s.clone(), *w)).collect());
                        card.state_time = st.last_time.as_ref().map(iso);
                    }
                    Ok(None) => {
                        card.advice = Some(
                            "holdings 未建账:首次 commit 在周一 15:35(周频 reb5)".into(),
                        );
                    }
                    Err(e) => {
                        let e_str = e.to_string();
                        card.status = "corrupt".into();
                        card.advice = Some(
                            crate::error::ErrorDto::from_anyhow(&anyhow::anyhow!(e_str))
                                .advice
                                .unwrap_or_else(|| {
                                    "state 异常:查看消息并考虑删除重建(重放幂等)".into()
                                }),
                        );
                    }
                },
                Err(e) => {
                    let e_str = e.clone();
                    card.status = "corrupt".into();
                    card.advice = Some(
                        crate::error::ErrorDto::from_anyhow(&anyhow::anyhow!(e_str))
                            .advice
                            .unwrap_or_else(|| {
                                "state 异常:查看消息并考虑删除重建(重放幂等)".into()
                            }),
                    );
                }
            }
        }
    }
    card
}

/// 账本3 今日清单 diff——直接采 sig_portfolio.json 的 trades(引擎已算好)。
pub fn read_portfolio_diff(ws: &Workspace, book: &Book) -> (Vec<DiffRowDto>, Option<String>) {
    match read_portfolio_sig(&book.sig_path(ws)) {
        Some((brief, rows)) => (rows, Some(brief.t)),
        None => (Vec::new(), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::books::BOOKS;
    use crate::paths::Workspace;
    use chrono::NaiveDateTime;
    use rquant::backtest::sim::SimAccount;
    use rquant::signal::{write_paper_state, PaperState};

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    /// tempdir 工作区 + 真树副本(读卡要靠树名校验 state)。
    fn fixture_ws() -> (tempfile::TempDir, Workspace) {
        let td = tempfile::tempdir().unwrap();
        let root = td.path().to_path_buf();
        std::fs::create_dir_all(root.join("paper")).unwrap();
        std::fs::create_dir_all(root.join("deploy")).unwrap();
        // 真树文件直接拷贝(测试在仓库内跑,引用真实 deploy 树保证 meta.name 同步)
        let repo = Workspace::detect(&std::env::current_dir().unwrap()).unwrap();
        for f in ["tree_v4_frozen.yaml", "strength_v1_frozen.yaml"] {
            std::fs::copy(repo.deploy_dir().join(f), root.join("deploy").join(f)).unwrap();
        }
        (td, Workspace::new(root))
    }

    #[test]
    fn empty_state_yields_empty_card() {
        let (_td, ws) = fixture_ws();
        let card = read_book_card(&ws, &BOOKS[0]);
        assert_eq!(card.status, "empty");
        assert!(card.nav.is_none());
    }

    #[test]
    fn committed_state_yields_ok_card_with_nav() {
        let (_td, ws) = fixture_ws();
        let tree = rquant::tree::loader::load_tree_file(&BOOKS[0].tree_path(&ws)).unwrap();
        let mut acc = SimAccount::default();
        acc.nav = 1.0539;
        let st = PaperState {
            version: 1,
            tree_name: tree.meta.name.clone(),
            last_time: Some(t("2026-06-11 15:00:00")),
            account: acc.snapshot(),
        };
        write_paper_state(&BOOKS[0].state_path(&ws), &st).unwrap();
        let card = read_book_card(&ws, &BOOKS[0]);
        assert_eq!(card.status, "ok");
        assert!((card.nav.unwrap() - 1.0539).abs() < 1e-12);
        assert_eq!(card.state_time.as_deref(), Some("2026-06-11T15:00:00"));
    }

    #[test]
    fn corrupt_state_yields_corrupt_card_with_advice() {
        let (_td, ws) = fixture_ws();
        std::fs::write(BOOKS[0].state_path(&ws), b"").unwrap(); // 空文件 = corrupt
        let card = read_book_card(&ws, &BOOKS[0]);
        assert_eq!(card.status, "corrupt");
        assert!(card.advice.is_some());
    }

    #[test]
    fn sig_json_feeds_last_signal_even_without_state() {
        let (_td, ws) = fixture_ws();
        // 真实形状:与引擎 SingleSignal 序列化字段一致
        let sig = serde_json::json!({
            "t": "2026-06-12T15:00:00", "target": 0.0, "current_pos": 0.0, "delta": 0.0,
            "reason": "tree", "leaf": "flat_wait",
            "paper": {"nav": 1.05, "total_return": 0.05, "max_drawdown": 0.02, "bars_replayed": 942}
        });
        std::fs::write(BOOKS[0].sig_path(&ws), serde_json::to_string(&sig).unwrap()).unwrap();
        let card = read_book_card(&ws, &BOOKS[0]);
        assert_eq!(card.status, "empty"); // state 仍未建
        let brief = card.last_signal.unwrap();
        assert_eq!(brief.bars_replayed, Some(942));
        assert_eq!(brief.leaf.as_deref(), Some("flat_wait"));
    }

    #[test]
    fn portfolio_card_and_diff_rows() {
        let (_td, ws) = fixture_ws();
        let b3 = &BOOKS[2];
        let tree = rquant::tree::loader::load_tree_file(&b3.tree_path(&ws)).unwrap();
        let mut holdings = std::collections::BTreeMap::new();
        holdings.insert("sh600900".to_string(), 0.5);
        holdings.insert("sz000333".to_string(), 0.5);
        let st = rquant::signal::HoldingsState {
            version: 1,
            tree_name: tree.meta.name.clone(),
            last_time: Some(t("2026-06-11 15:00:00")),
            holdings,
        };
        rquant::signal::write_holdings_state(&b3.state_path(&ws), &st).unwrap();
        let sig = serde_json::json!({
            "t": "2026-06-12T15:00:00", "n_fresh": 10,
            "targets": [["sh600900", 0.5], ["sz000333", 0.5]],
            "trades": [
                {"symbol": "sh600900", "action": "Hold", "from_w": 0.5, "to_w": 0.5},
                {"symbol": "sz000333", "action": "Hold", "from_w": 0.5, "to_w": 0.5}
            ]
        });
        std::fs::write(b3.sig_path(&ws), serde_json::to_string(&sig).unwrap()).unwrap();
        let card = read_book_card(&ws, b3);
        assert_eq!(card.status, "ok");
        assert_eq!(card.holdings.as_ref().unwrap().len(), 2);
        let (rows, t_opt) = read_portfolio_diff(&ws, b3);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].action, "Hold");
        assert_eq!(t_opt.as_deref(), Some("2026-06-12T15:00:00"));
    }

    #[test]
    fn snapshot_dto_mirrors_all_13_fields() {
        let acc = SimAccount::default();
        let snap = acc.snapshot();
        let dto = snapshot_to_dto(&snap);
        assert_eq!(dto.pos, 0.0);
        assert!(dto.entry_price.is_none()); // default NaN → None
    }

    #[test]
    fn trade_action_debug_strings_are_locked() {
        use rquant::signal::TradeAction;
        assert_eq!(format!("{:?}", TradeAction::Buy), "Buy");
        assert_eq!(format!("{:?}", TradeAction::Sell), "Sell");
        assert_eq!(format!("{:?}", TradeAction::Adjust), "Adjust");
        assert_eq!(format!("{:?}", TradeAction::Hold), "Hold");
    }

    #[test]
    fn corrupt_holdings_state_yields_corrupt_card_with_advice() {
        let (_td, ws) = fixture_ws();
        let b3 = &BOOKS[2];
        std::fs::write(b3.state_path(&ws), b"").unwrap(); // 空文件 = corrupt
        let card = read_book_card(&ws, b3);
        assert_eq!(card.status, "corrupt");
        assert!(card.advice.as_deref().unwrap_or("").contains("删除"));
    }
}
