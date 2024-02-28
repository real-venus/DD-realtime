use serde_derive::{Deserialize, Serialize};
use sqlx::types::Decimal;
use std::collections::HashMap;

#[derive(Deserialize, Serialize, Debug)]
pub struct Market {
    pub name: String,
    pub slug: String,
    // Add other fields as needed
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy)]
pub struct TradeData {
    pub price: f64,

    pub amount: f64,

    pub market_buy: bool,

    pub timestamp: u64,
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy)]
pub struct LastTradeData {
    pub price: f64,

    pub amount: f64,

    #[serde(rename = "marketBuy")]
    pub market_buy: bool,

    pub timestamp: u64,
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq)]
pub struct SummaryData {
    #[serde(rename = "change24H")]
    pub change_24h: f64,

    #[serde(rename = "price24H")]
    pub price_24h: f64,

    #[serde(rename = "high24H")]
    pub high_24h: f64,

    #[serde(rename = "low24H")]
    pub low_24h: f64,

    #[serde(rename = "volume24H")]
    pub volume_24h: f64,

    pub price: f64,

    #[serde(rename = "solPrice")]
    pub sol_price: f64,

    #[serde(rename = "nftPool")]
    pub nft_pool: Option<f64>,

    #[serde(rename = "lotSupply")]
    pub lot_supply: Option<f64>,

    #[serde(rename = "marketBuy")]
    pub market_buy: Option<bool>,
}

#[derive(Deserialize, Serialize, Debug, PartialEq, Clone)]
pub struct MarketPricesData {
    #[serde(rename = "marketPrices")]
    pub market_prices: HashMap<String, PriceData>,
}

#[derive(Deserialize, Serialize, Debug, PartialEq, Clone)]
pub struct PriceData {
    pub price: f64,

    #[serde(rename = "marketBuy")]
    pub market_buy: bool,

    #[serde(rename = "change24H")]
    pub change_24h: f64,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct MarketsResponse {
    pub message: Vec<Market>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct SummaryResponse {
    pub message: SummaryData,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct PricesResponse {
    pub message: HashMap<String, PriceData>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy)]
pub struct SummaryPublishData {
    pub summary: SummaryData,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct PublishAllData<F> {
    #[serde(rename = "type")]
    pub _type: String,

    pub market: String,

    pub data: F,

    pub id: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct PublishUidData<F> {
    #[serde(rename = "type")]
    pub _type: u64,

    pub market: String,

    pub data: F,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct MarketConfig {
    pub name: String,
    pub slug: String,
    pub ob_market_address: Option<String>,
    pub gd_market_address: Option<String>,
    pub base_decimals: u8,
    pub quote_decimals: u8,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketTrade {
    pub slug: String,
    pub order_id: Option<String>,
    pub market_buy: u8,
    pub avg_price: Decimal,
    pub amount: Decimal,
    pub timestamp: u64,
    pub market_address: String,
    pub blocktime: u64,
    pub index: u64,
    pub avg_price_lots: Decimal,
    pub amount_lots: Decimal,
    pub slot: u64,
    pub transaction_signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventData {
    pub event: String,
    pub user: String,
    pub amount: Decimal,
    pub price: Decimal,
    pub tx: String,
    pub market: String,
    pub filled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradePublishData {
    pub amount: f64,
    pub price: f64,
    #[serde(rename = "priceLots")]
    pub price_lots: f64,
    #[serde(rename = "amountLots")]
    pub amount_lots: f64,
    #[serde(rename = "marketBuy")]
    pub market_buy: bool,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradesPublishData {
    pub trades: Vec<TradePublishData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandleData {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub amount: f64,
    pub begin_ts: u64,
    pub end_ts: u64,
    pub unit: String,
    pub slug: String,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct MarketOrder {
    pub price: f64,
    pub amount: f64,

    #[serde(rename = "priceLots")]
    pub price_lots: u64,

    #[serde(rename = "sizeLots")]
    pub size_lots: u64,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct MarketOrders {
    pub asks: Vec<MarketOrder>,
    pub bids: Vec<MarketOrder>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct MarketSendData {
    #[serde(rename = "orderBook")]
    pub order_book: MarketOrders,

    pub slot: u64,
}
