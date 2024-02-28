use std::num::NonZeroUsize;

use num_traits::ToPrimitive;
use postgrest::Postgrest;
use redis::{Client, Commands, Connection, RedisError};
use sqlx::types::Decimal;

use crate::{
    api::get_summary,
    constants::{CHANNEL_NAME, PRICES_KEY, SUMMARY_KEY},
    insert_candles, insert_trades,
    structs::market::{
        LastTradeData, MarketOrders, MarketPricesData, MarketSendData, MarketTrade, PriceData,
        SummaryPublishData, TradeData, TradePublishData, TradesPublishData,
    },
    utils::generate_publish_data,
};

/*
 * Function: update_trades
 * 1. Update redis's last_trade_data with provided trades
 * 2. Extend redis's recent_trades with current trades
 * 3. Publish trade updates to redis clients
 * 4. Insert trades data into supabase's trade table
 * 5. Publish price updates using gigadexV2 api
 * 6. Insert candle data based on trade data
 */
pub async fn update_trades(
    api_url: String,
    redis_client: Client,
    supabase_client: Postgrest,
    trades: Vec<MarketTrade>,
) -> anyhow::Result<()> {
    let mut redis_conn = redis_client.get_connection().unwrap();

    let first_trade = trades.first().unwrap();
    let market_slug = first_trade.slug.clone();
    let market_address = first_trade.market_address.clone();

    let trade_datas: Vec<TradeData> = trades
        .iter()
        .map(|x| TradeData {
            price: Decimal::to_f64(&x.avg_price).unwrap(),
            amount: Decimal::to_f64(&x.amount).unwrap(),
            market_buy: x.market_buy == 1,
            timestamp: x.timestamp,
        })
        .collect();

    // Update last trade data
    let last_trade = trade_datas.last().unwrap();
    let _: Result<String, RedisError> = redis_conn.set(
        format!("last_trade_data:{}", market_slug),
        serde_json::to_string(&LastTradeData {
            price: last_trade.price,
            amount: last_trade.amount,
            market_buy: last_trade.market_buy,
            timestamp: last_trade.timestamp,
        })
        .unwrap(),
    );

    // Update recent trades
    let recent_trades_data: Vec<String> = redis_conn
        .lrange(format!("recent_trades:{}", market_address), 0, -1)
        .unwrap();
    let mut recent_trades: Vec<TradeData> = recent_trades_data
        .iter()
        .map(|x| serde_json::from_str::<TradeData>(x).unwrap())
        .collect();
    recent_trades.extend(trade_datas.clone());

    let trades_len: usize = recent_trades.len();
    if trades_len > 100 {
        redis_conn.rpop(
            format!("recent_trades:{}", market_address),
            NonZeroUsize::new(trades_len - 100),
        )?;
    }

    redis_conn.lpush::<String, Vec<String>, _>(
        format!("recent_trades:{}", market_address),
        trade_datas
            .iter()
            .map(|x| serde_json::to_string(x).unwrap())
            .collect(),
    )?;

    // Broadcast trade update
    let trades_publish_array = trades
        .iter()
        .map(|x| TradePublishData {
            price: Decimal::to_f64(&x.avg_price).unwrap_or_default(),
            amount: Decimal::to_f64(&x.amount).unwrap_or_default(),
            price_lots: Decimal::to_f64(&x.avg_price_lots).unwrap_or_default(),
            amount_lots: Decimal::to_f64(&x.amount_lots).unwrap_or_default(),
            market_buy: x.market_buy == 1,
            timestamp: x.timestamp,
        })
        .collect();
    redis_conn.publish(
        CHANNEL_NAME,
        generate_publish_data(
            &market_slug,
            &TradesPublishData {
                trades: trades_publish_array,
            },
            first_trade.order_id.clone(),
        ),
    )?;

    // Insert trade record
    insert_trades(supabase_client.clone(), trades.clone()).await?;

    // Publish summary data
    let summary = get_summary(&api_url, &market_slug).await.unwrap();
    redis_conn.set(
        format!("{}:{}", SUMMARY_KEY, market_slug),
        &serde_json::to_string(&SummaryPublishData { summary }).unwrap(),
    )?;
    redis_conn.publish(
        CHANNEL_NAME,
        generate_publish_data(&market_slug, &SummaryPublishData { summary }, None),
    )?;

    // Publish price data
    let prices_str: String = redis_conn.get(PRICES_KEY)?;
    let mut prices_data = serde_json::from_str::<MarketPricesData>(prices_str.as_str())?;
    match prices_data.market_prices.get_mut(&market_slug) {
        Some(market_price) => {
            market_price.price = last_trade.price;
            market_price.market_buy = last_trade.market_buy;
            market_price.change_24h = summary.change_24h;
        }
        None => {
            prices_data.market_prices.insert(
                market_slug,
                PriceData {
                    price: last_trade.price,
                    market_buy: last_trade.market_buy,
                    change_24h: summary.change_24h,
                },
            );
        }
    }

    redis_conn.set(PRICES_KEY, serde_json::to_string(&prices_data).unwrap())?;

    redis_conn.publish(
        CHANNEL_NAME,
        generate_publish_data("general", &prices_data, None),
    )?;

    // Insert candles
    for unit in ["1m", "15m", "4h", "1d"] {
        tokio::spawn({
            let supabase_clone = supabase_client.clone();
            let trades_clone = trades.clone();

            async move {
                let _ = insert_candles(supabase_clone, trades_clone, unit).await;
            }
        });
    }

    Ok(())
}

pub fn publish_trades_data(
    market: &String,
    market_state: &MarketOrders,
    redis_conn: &mut Connection,
    slot: u64,
) -> anyhow::Result<()> {
    let send_data = MarketSendData {
        order_book: market_state.clone(),
        slot,
    };

    redis_conn.set(
        format!("compressed_orderbook:{}", market),
        serde_json::to_string(&send_data)?,
    )?;

    let publish_string = generate_publish_data(&market, &send_data, None);
    redis_conn.publish(CHANNEL_NAME, publish_string)?;

    Ok(())
}
