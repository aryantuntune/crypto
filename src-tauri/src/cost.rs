use crate::db::Db;
use crate::error::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy)]
pub struct ModelRates {
    pub input_per_mtok: f64,
    pub cached_per_mtok: f64,
    pub output_per_mtok: f64,
}

pub fn rates_for(model: &str) -> ModelRates {
    // Sonnet 4.6: $3/$0.30/$15. Opus 4.7: $15/$1.50/$75. Haiku 4.5: $1/$0.10/$5.
    if model.contains("opus") {
        ModelRates { input_per_mtok: 15.0, cached_per_mtok: 1.5, output_per_mtok: 75.0 }
    } else if model.contains("haiku") {
        ModelRates { input_per_mtok: 1.0, cached_per_mtok: 0.10, output_per_mtok: 5.0 }
    } else {
        ModelRates { input_per_mtok: 3.0, cached_per_mtok: 0.30, output_per_mtok: 15.0 }
    }
}

pub fn cost_usd(rates: ModelRates, input: u64, cached: u64, output: u64) -> f64 {
    let m = 1_000_000.0;
    (input as f64 * rates.input_per_mtok
        + cached as f64 * rates.cached_per_mtok
        + output as f64 * rates.output_per_mtok) / m
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DailyCost {
    pub date: String,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}

pub fn add_usage(db: &Db, model: &str, input: u64, cached: u64, output: u64) -> Result<DailyCost> {
    let date = Utc::now().format("%Y-%m-%d").to_string();
    let added = cost_usd(rates_for(model), input, cached, output);
    let conn = db.lock().unwrap();
    conn.execute(
        "INSERT INTO cost_daily(date, input_tokens, cached_input_tokens, output_tokens, cost_usd)
         VALUES(?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(date) DO UPDATE SET
            input_tokens = input_tokens + excluded.input_tokens,
            cached_input_tokens = cached_input_tokens + excluded.cached_input_tokens,
            output_tokens = output_tokens + excluded.output_tokens,
            cost_usd = cost_usd + excluded.cost_usd",
        rusqlite::params![date, input as i64, cached as i64, output as i64, added],
    )?;
    let row = conn.query_row(
        "SELECT date, input_tokens, cached_input_tokens, output_tokens, cost_usd FROM cost_daily WHERE date=?1",
        [&date],
        |r| Ok(DailyCost {
            date: r.get(0)?,
            input_tokens: r.get::<_, i64>(1)? as u64,
            cached_input_tokens: r.get::<_, i64>(2)? as u64,
            output_tokens: r.get::<_, i64>(3)? as u64,
            cost_usd: r.get(4)?,
        })
    )?;
    Ok(row)
}

pub fn today(db: &Db) -> Result<DailyCost> {
    let date = Utc::now().format("%Y-%m-%d").to_string();
    let conn = db.lock().unwrap();
    let row = conn.query_row(
        "SELECT date, input_tokens, cached_input_tokens, output_tokens, cost_usd FROM cost_daily WHERE date=?1",
        [&date],
        |r| Ok(DailyCost {
            date: r.get(0)?,
            input_tokens: r.get::<_, i64>(1)? as u64,
            cached_input_tokens: r.get::<_, i64>(2)? as u64,
            output_tokens: r.get::<_, i64>(3)? as u64,
            cost_usd: r.get(4)?,
        })
    ).unwrap_or(DailyCost { date, ..Default::default() });
    Ok(row)
}

pub fn is_over_cap(today_cost: f64, cap: f64) -> bool { today_cost >= cap }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn cost_arithmetic_sonnet() {
        let r = rates_for("claude-sonnet-4-6");
        // 1M input + 0 cached + 0 output = $3
        assert!((cost_usd(r, 1_000_000, 0, 0) - 3.0).abs() < 1e-9);
        // 0 input + 0 cached + 1M output = $15
        assert!((cost_usd(r, 0, 0, 1_000_000) - 15.0).abs() < 1e-9);
        // 1M cached = $0.30
        assert!((cost_usd(r, 0, 1_000_000, 0) - 0.30).abs() < 1e-9);
    }

    #[test]
    fn rates_pick_correct_model() {
        assert_eq!(rates_for("claude-haiku-4-5-20251001").input_per_mtok, 1.0);
        assert_eq!(rates_for("claude-opus-4-7").input_per_mtok, 15.0);
        assert_eq!(rates_for("claude-sonnet-4-6").input_per_mtok, 3.0);
    }

    #[test]
    fn add_usage_accumulates_same_day() {
        let tmp = tempfile::tempdir().unwrap();
        let db = db::open(&tmp.path().join("chat.db")).unwrap();
        add_usage(&db, "claude-sonnet-4-6", 1000, 0, 200).unwrap();
        let r = add_usage(&db, "claude-sonnet-4-6", 500, 100, 50).unwrap();
        assert_eq!(r.input_tokens, 1500);
        assert_eq!(r.cached_input_tokens, 100);
        assert_eq!(r.output_tokens, 250);
        assert!(r.cost_usd > 0.0);
    }

    #[test]
    fn cap_check() {
        assert!(!is_over_cap(1.0, 2.0));
        assert!(is_over_cap(2.0, 2.0));
        assert!(is_over_cap(2.5, 2.0));
    }
}
