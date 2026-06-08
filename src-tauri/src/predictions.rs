use crate::coingecko;
use crate::db::Db;
use crate::error::Result;
use crate::llm::types::{Action, AnalysisJson};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prediction {
    pub id: i64,
    pub ts: i64,
    pub symbol: String,
    pub horizon_seconds: i64,
    pub predicted_prob_up: f64,
    pub recommended_action: String,
    pub stop_loss_pct: Option<f64>,
    pub take_profit_pct: Option<f64>,
    pub citations_json: Option<String>,
    pub outcome_status: String,
    pub outcome_price_at_ts: Option<f64>,
    pub outcome_price_at_horizon: Option<f64>,
    pub outcome_resolved_at: Option<i64>,
}

pub fn insert_from_analysis(db: &Db, symbol: &str, a: &AnalysisJson) -> Result<i64> {
    let ts = chrono::Utc::now().timestamp();
    let action = match a.action { Action::Buy => "buy", Action::Sell => "sell", Action::Hold => "hold" };
    let cit = serde_json::to_string(&a.citations).ok();
    let conn = db.lock().unwrap();
    conn.execute(
        "INSERT INTO predictions(ts, symbol, horizon_seconds, predicted_prob_up, recommended_action, stop_loss_pct, take_profit_pct, citations_json)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        rusqlite::params![
            ts, symbol, a.horizon.seconds(),
            a.probability_up as f64, action,
            a.stop_loss_pct.map(|f| f as f64), a.take_profit_pct.map(|f| f as f64),
            cit
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn pending_due(db: &Db) -> Result<Vec<Prediction>> {
    let now = chrono::Utc::now().timestamp();
    let conn = db.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, ts, symbol, horizon_seconds, predicted_prob_up, recommended_action, stop_loss_pct, take_profit_pct, citations_json, outcome_status, outcome_price_at_ts, outcome_price_at_horizon, outcome_resolved_at
         FROM predictions WHERE outcome_status='pending' AND (ts + horizon_seconds) <= ?1"
    )?;
    let rows = stmt.query_map([now], |r| Ok(Prediction {
        id: r.get(0)?, ts: r.get(1)?, symbol: r.get(2)?, horizon_seconds: r.get(3)?,
        predicted_prob_up: r.get(4)?, recommended_action: r.get(5)?,
        stop_loss_pct: r.get(6)?, take_profit_pct: r.get(7)?,
        citations_json: r.get(8)?, outcome_status: r.get(9)?,
        outcome_price_at_ts: r.get(10)?, outcome_price_at_horizon: r.get(11)?,
        outcome_resolved_at: r.get(12)?,
    }))?;
    Ok(rows.collect::<std::result::Result<_,_>>()?)
}

pub async fn resolve_one(db: &Db, p: &Prediction) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    let price_now_res = coingecko::get_usd_price(&p.symbol).await;
    match price_now_res {
        Ok(price_now) => {
            let conn = db.lock().unwrap();
            conn.execute(
                "UPDATE predictions SET outcome_status='resolved', outcome_price_at_horizon=?1, outcome_resolved_at=?2 WHERE id=?3",
                rusqlite::params![price_now, now, p.id]
            )?;
        }
        Err(_) => {
            let conn = db.lock().unwrap();
            conn.execute("UPDATE predictions SET outcome_status='failed', outcome_resolved_at=?1 WHERE id=?2",
                rusqlite::params![now, p.id])?;
        }
    }
    Ok(())
}

pub fn record_initial_price(db: &Db, prediction_id: i64, price: f64) -> Result<()> {
    let conn = db.lock().unwrap();
    conn.execute("UPDATE predictions SET outcome_price_at_ts=?1 WHERE id=?2",
        rusqlite::params![price, prediction_id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::llm::types::{Citation, Horizon};

    #[test]
    fn insert_then_pending_due() {
        let tmp = tempfile::tempdir().unwrap();
        let db = db::open(&tmp.path().join("c.db")).unwrap();
        let a = AnalysisJson {
            action: Action::Buy, probability_up: 0.6, horizon: Horizon::H1,
            stop_loss_pct: Some(2.0), take_profit_pct: Some(4.0),
            key_signals: vec!["x".into()],
            citations: vec![Citation { doc: "doc.pdf".into(), page: Some(1) }],
        };
        let id = insert_from_analysis(&db, "BTCUSDT", &a).unwrap();
        // not yet due
        assert!(pending_due(&db).unwrap().is_empty());
        // shift its ts back so it becomes due
        {
            let conn = db.lock().unwrap();
            conn.execute("UPDATE predictions SET ts = ts - 7200 WHERE id=?1", [id]).unwrap();
        }
        assert_eq!(pending_due(&db).unwrap().len(), 1);
    }
}
