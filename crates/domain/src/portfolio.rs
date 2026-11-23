//! Read-only portfolio and position performance analytics.
//!
//! Faithful port of legacy `simulation/portfolio.mjs`.
//! Computes equity, realized/unrealized PnL, margin utilization, win rate, and
//! asset exposure across open and closed positions.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PositionStatus {
    Open,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub id: String,
    pub symbol: String,
    pub side: String, // "BUY" or "SELL"
    pub qty: f64,
    pub entry_price: f64,
    pub exit_price: Option<f64>,
    pub status: PositionStatus,
    pub pnl_abs: Option<f64>,
    pub fees: Option<f64>,
    pub leverage: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortfolioSummary {
    pub base: f64,
    pub equity: f64,
    pub realized: f64,
    pub unrealized: f64,
    pub fees: f64,
    pub open_count: usize,
    pub closed_count: usize,
    pub wins: usize,
    pub losses: usize,
    pub win_rate: Option<f64>,
    pub exposure: f64,
    pub margin_used: f64,
    pub free_margin: f64,
    pub exposure_by_symbol: BTreeMap<String, f64>,
}

/// Calculates complete account equity, PnL, margin, and exposure metrics.
#[must_use]
pub fn calculate_portfolio_summary(
    base: f64,
    positions: &[Position],
    current_prices: &BTreeMap<String, f64>,
) -> PortfolioSummary {
    let mut realized = 0.0;
    let mut unrealized = 0.0;
    let mut fees = 0.0;
    let mut wins = 0;
    let mut losses = 0;
    let mut open_count = 0;
    let mut closed_count = 0;
    let mut margin_used = 0.0;
    let mut exposure_by_symbol: BTreeMap<String, f64> = BTreeMap::new();

    for p in positions {
        if let Some(f) = p.fees {
            fees += f;
        }

        match p.status {
            PositionStatus::Closed => {
                closed_count += 1;
                if let Some(pnl) = p.pnl_abs {
                    realized += pnl;
                    if pnl > 0.0 {
                        wins += 1;
                    } else if pnl < 0.0 {
                        losses += 1;
                    }
                }
            }
            PositionStatus::Open => {
                open_count += 1;
                let notional = (p.qty * p.entry_price).abs();
                *exposure_by_symbol.entry(p.symbol.clone()).or_insert(0.0) += notional;

                let lev = p.leverage.max(1.0);
                margin_used += notional / lev;

                if let Some(&curr_px) = current_prices.get(&p.symbol) {
                    let mark_pnl = if p.side.eq_ignore_ascii_case("BUY") {
                        p.qty * (curr_px - p.entry_price)
                    } else {
                        p.qty * (p.entry_price - curr_px)
                    };
                    unrealized += mark_pnl;
                }
            }
        }
    }

    let equity = base + realized + unrealized;
    let total_closed = wins + losses;
    let win_rate = if total_closed > 0 {
        Some(wins as f64 / total_closed as f64)
    } else {
        None
    };

    let exposure: f64 = exposure_by_symbol.values().sum();
    let free_margin = equity - margin_used;

    PortfolioSummary {
        base,
        equity: (equity * 100.0).round() / 100.0,
        realized: (realized * 100.0).round() / 100.0,
        unrealized: (unrealized * 100.0).round() / 100.0,
        fees: (fees * 100.0).round() / 100.0,
        open_count,
        closed_count,
        wins,
        losses,
        win_rate: win_rate.map(|w| (w * 10_000.0).round() / 100.0), // Percentage e.g. 60.0%
        exposure: (exposure * 100.0).round() / 100.0,
        margin_used: (margin_used * 100.0).round() / 100.0,
        free_margin: (free_margin * 100.0).round() / 100.0,
        exposure_by_symbol,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculates_portfolio_summary_correctly() {
        let positions = vec![
            Position {
                id: "pos1".to_owned(),
                symbol: "BTCUSDT".to_owned(),
                side: "BUY".to_owned(),
                qty: 1.0,
                entry_price: 60000.0,
                exit_price: Some(62000.0),
                status: PositionStatus::Closed,
                pnl_abs: Some(2000.0),
                fees: Some(10.0),
                leverage: 10.0,
            },
            Position {
                id: "pos2".to_owned(),
                symbol: "ETHUSDT".to_owned(),
                side: "BUY".to_owned(),
                qty: 5.0,
                entry_price: 3000.0,
                exit_price: Some(2800.0),
                status: PositionStatus::Closed,
                pnl_abs: Some(-1000.0),
                fees: Some(5.0),
                leverage: 5.0,
            },
            Position {
                id: "pos3".to_owned(),
                symbol: "SOLUSDT".to_owned(),
                side: "BUY".to_owned(),
                qty: 100.0,
                entry_price: 150.0,
                exit_price: None,
                status: PositionStatus::Open,
                pnl_abs: None,
                fees: Some(2.0),
                leverage: 10.0,
            },
        ];

        let mut prices = BTreeMap::new();
        prices.insert("SOLUSDT".to_owned(), 160.0); // +$10/unit * 100 = +$1000 unrealized

        let summary = calculate_portfolio_summary(10_000.0, &positions, &prices);

        assert_eq!(summary.closed_count, 2);
        assert_eq!(summary.open_count, 1);
        assert_eq!(summary.wins, 1);
        assert_eq!(summary.losses, 1);
        assert_eq!(summary.win_rate, Some(50.0)); // 50%
        assert_eq!(summary.realized, 1000.0); // 2000 - 1000
        assert_eq!(summary.unrealized, 1000.0); // 100 * (160 - 150)
        assert_eq!(summary.equity, 12000.0); // 10000 + 1000 + 1000
        assert_eq!(summary.fees, 17.0); // 10 + 5 + 2
        assert_eq!(summary.margin_used, 1500.0); // 100 * 150 / 10
        assert_eq!(summary.free_margin, 10500.0); // 12000 - 1500
    }
}
