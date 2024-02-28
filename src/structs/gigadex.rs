use anchor_lang::prelude::*;
use anchor_lang::{AnchorDeserialize, AnchorSerialize};
use bytemuck::{Pod, Zeroable};
use serde_derive::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

pub const ORDERBOOK_DEPTH: usize = 1000; // this is before any compression
pub const MAX_FILLS_PER_MARKET_ORDER: usize = 64;
pub const USERS_PER_MARKET: usize = 10_000;

#[derive(Debug, Clone, Default)]
pub struct GdMarketInfo {
    pub name: String,
    pub address: Pubkey,
    pub bids: Pubkey,
    pub asks: Pubkey,
    pub balances: Pubkey,
    pub buy_order_log: Pubkey,
    pub sell_order_log: Pubkey,
    pub base_decimals: u8,
    pub quote_decimals: u8,
    pub multiplier: u64,
}
impl GdMarketInfo {
    pub fn is_valid_account(&self, account: &Pubkey) -> bool {
        account.eq(&self.bids)
            || account.eq(&self.asks)
            || account.eq(&self.balances)
            || account.eq(&self.buy_order_log)
            || account.eq(&self.sell_order_log)
    }
}

#[derive(AnchorDeserialize, AnchorSerialize, Debug, Clone)]
pub struct GdMarketState {
    pub mint: Pubkey,
    pub balances: Pubkey,
    pub wsol_vault: Pubkey,
    pub lot_vault: Pubkey,
    pub asks: Pubkey,
    pub bids: Pubkey,
}

#[derive(Deserialize, Serialize, Default, Debug, Clone, PartialEq)]
pub struct GdMarketOrder {
    pub uid: u64,
    pub price_lots: u64,
    pub amount_lots: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct FilledOrder {
    pub price: u64,
    pub amount: u64,
    pub uid: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct Node {
    // key
    pub price: u64,

    // order meta
    pub amount: u64,
    pub uid: u64,

    // indexes
    pub left: u64,
    pub right: u64,
    pub next: u64,

    // balance meta
    pub height: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct NodeDeltaLog {
    pub key: u64,
    pub is_delete: u64,
    pub is_insert: u64,
    pub is_delta: u64,
    pub amount: u64,
    pub uid: u64,
    pub price: u64,
}

#[derive(Clone, Copy, Debug)]
#[repr(packed)]
#[allow(dead_code)]
pub struct OrderTree {
    pub root_idx: u64,
    pub market_buy: u64,

    pub nodes: [Node; ORDERBOOK_DEPTH], // Leaves the account at 10,485,680 bytes.
    pub num_orders: u64,
    pub current_signer: Pubkey,

    // match state
    pub remaining_amount: u64,
    pub num_fills: u64,

    pub fills: [FilledOrder; MAX_FILLS_PER_MARKET_ORDER],

    pub num_deltas: u64,
    pub node_delta: [NodeDeltaLog; MAX_FILLS_PER_MARKET_ORDER],

    pub amount_cancelled: u64,
}
unsafe impl Zeroable for OrderTree {}
unsafe impl Pod for OrderTree {}

#[derive(Clone, Copy, Debug)]
pub struct Entry {
    pub lamports: u64,
    pub lots: u64,
}

#[derive(Clone, Copy, Debug)]
#[repr(packed)]
#[allow(dead_code)]
pub struct UserBalances {
    pub num_users: u64,
    pub entries: [Entry; USERS_PER_MARKET],
}
unsafe impl Zeroable for UserBalances {}
unsafe impl Pod for UserBalances {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GdBalance {
    pub lamports: f64,
    pub lots: f64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Debug, Clone, PartialEq)]
pub struct GdMarketOrderLog {
    pub amount: u64,
    pub total_value_lamports: u64,
    pub counter: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GdOrderData {
    pub amount: f64,
    pub price: f64,
    #[serde(rename = "priceLots")]
    pub price_lots: u64,
    #[serde(rename = "amountLots")]
    pub amount_lots: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GdAsksData {
    #[serde(rename = "uidAsks")]
    pub uid_asks: Vec<GdOrderData>,
    pub slot: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GdBidsData {
    #[serde(rename = "uidBids")]
    pub uid_bids: Vec<GdOrderData>,
    pub slot: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GdBalanceData {
    #[serde(rename = "claimableBalance")]
    pub claimable_balance: GdBalance,
    pub slot: u64,
}
