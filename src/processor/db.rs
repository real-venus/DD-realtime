use std::collections::HashMap;

use num_traits::ToPrimitive;
use postgrest::Postgrest;
use sqlx::types::Decimal;

use crate::{
    constants::{SECONDS_PER_DAY, SECONDS_PER_HOUR, SECONDS_PER_MINUTE},
    structs::market::{CandleData, EventData, MarketTrade},
};

/*
 * Function: insert_candles
 * 1. Build candle data based on unit and trades data
 * 2. Insert candle records into supabase
 */
pub async fn insert_candles(
    supabase_client: Postgrest,
    trades: Vec<MarketTrade>,
    unit: &str,
) -> anyhow::Result<()> {
    let market_slug = trades.first().unwrap().slug.clone();
    let blocktime = trades.first().unwrap().blocktime;

    let delta_secs = match unit {
        "1m" => SECONDS_PER_MINUTE,
        "15m" => SECONDS_PER_MINUTE * 15,
        "4h" => SECONDS_PER_HOUR * 4,
        "1d" => SECONDS_PER_DAY,
        _ => 60,
    };

    let mut candle_set: HashMap<u64, CandleData> = HashMap::new();
    for trade in trades {
        let begin_ts = (blocktime / delta_secs) * delta_secs;
        let end_ts = begin_ts + delta_secs;
        let price = Decimal::to_f64(&trade.avg_price).unwrap_or_default();
        let amount = Decimal::to_f64(&trade.amount).unwrap_or_default();

        if !candle_set.contains_key(&begin_ts) {
            let mut open_price: f64 = 0.0;
            if candle_set.len() > 0 {
                let mut ts_keys: Vec<u64> = candle_set.keys().map(|x| *x).collect();
                ts_keys.sort();
                ts_keys.reverse();
                for ts in ts_keys {
                    if ts < begin_ts {
                        open_price = candle_set.get(&ts).unwrap().close;
                        break;
                    }
                }
            };

            if open_price == 0.0 {
                let resp = supabase_client
                    .from("tb_market_candles")
                    .select("*")
                    .eq("slug", market_slug.clone())
                    .eq("unit", unit)
                    .lt("begin_ts", begin_ts.to_string())
                    .order("begin_ts.desc")
                    .limit(1)
                    .execute()
                    .await;

                if resp.is_ok() {
                    let data = resp?.text().await?;
                    match serde_json::from_str::<Vec<CandleData>>(&data) {
                        Ok(previous_candle) => {
                            if previous_candle.len() > 0 {
                                open_price = previous_candle.first().unwrap().close;
                            }
                        }
                        Err(_) => {}
                    }
                }
            };

            if open_price == 0.0 {
                open_price = price;
            }

            let candle = CandleData {
                open: open_price,
                high: price,
                low: price,
                close: price,
                amount,
                begin_ts,
                end_ts,
                unit: unit.to_string(),
                slug: trade.slug,
            };
            candle_set.insert(begin_ts, candle);
        } else {
            let candle = candle_set.get_mut(&begin_ts).unwrap();
            candle.amount += amount;
            candle.high = f64::max(candle.high, price);
            candle.low = f64::min(candle.low, price);
            candle.close = price;
        }
    }

    // Insert candle records
    let candles_data: Vec<CandleData> = candle_set.values().map(|x| x.clone()).collect();
    supabase_client
        .from("tb_market_candles")
        .insert(serde_json::to_string(&candles_data).unwrap())
        .on_conflict("slug, begin_ts, unit")
        .execute()
        .await?;

    Ok(())
}

/*
 * Function: insert_trades
 * 1. Insert trade records into supabase
 */
pub async fn insert_trades(
    supabase_client: Postgrest,
    trades: Vec<MarketTrade>,
) -> anyhow::Result<()> {
    supabase_client
        .from("tb_market_trades")
        .insert(serde_json::to_string(&trades).unwrap())
        .execute()
        .await?;

    Ok(())
}

/*
 * Function: insert_events
 * 1. Insert ask/bid/fill events into supabase
 */
pub async fn insert_events(
    supabase_client: Postgrest,
    events: Vec<EventData>,
) -> anyhow::Result<()> {
    supabase_client
        .from("tb_events")
        .insert(serde_json::to_string(&events).unwrap())
        .execute()
        .await?;

    Ok(())
}
