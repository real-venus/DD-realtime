use openbook_dex::{
    matching::Side,
    state::{strip_header, Event, EventQueueHeader, EventView, Queue},
};
use postgrest::Postgrest;
use redis::{Client, Connection};
use solana_sdk::account_info::AccountInfo;
use sqlx::types::Decimal;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{
    collections::{HashMap, HashSet},
    error::Error,
    str::FromStr,
};

use anchor_lang::AnchorDeserialize;
use solana_account_decoder::UiAccountEncoding;
use solana_client::{nonblocking::rpc_client::RpcClient, rpc_config::RpcAccountInfoConfig};
use solana_sdk::{commitment_config::CommitmentConfig, program_pack::Pack, pubkey::Pubkey};

use crate::{
    processor::market::{publish_trades_data, update_trades},
    structs::{
        geyser::Account,
        market::{MarketConfig, MarketOrder, MarketOrders, MarketTrade},
        mint::Mint,
        openbook::{ObMarketInfo, ObMarketState},
        slab::{construct_levels, Slab},
    },
    utils::{array_to_pubkey, token_factor},
};

/*
 * Function: parse_openbook_account
 * 1. Parse account data from geyser subscribe
 * 2. If ask/bids account, then update orderbook data and publish compressed_orderbook
 * 3. If fill account, then build trades data with price/amount calculation and call update_trades
 */
pub async fn parse_openbook_account(
    api_url: String,
    redis_client: Client,
    supabase_client: Postgrest,
    market: ObMarketInfo,
    account: &mut Account,
    redis_conn: &mut Connection,
    market_orders: &mut HashMap<String, MarketOrders>,
    filled_order_ids: &mut HashSet<u128>,
) -> Result<(), Box<dyn Error>> {
    // Built account_info for parse data
    let account_info = AccountInfo::new(
        &account.pubkey,
        false,
        false,
        &mut account.lamports,
        &mut account.data,
        &account.owner,
        account.executable,
        account.rent_epoch,
    );
    let market_state = market_orders.get_mut(&market.address.to_string()).unwrap();

    if market.event_queue.eq(&account.pubkey) {
        let ret = strip_header::<EventQueueHeader, Event>(&account_info, false).unwrap();
        let mut trades_to_insert: Vec<MarketTrade> = Vec::new();
        let events = Queue::new(ret.0, ret.1);

        // Parse events
        for event in events.iter() {
            let view = event.as_view()?;
            match view {
                // Process fill event only
                EventView::Fill {
                    side,
                    maker,
                    native_qty_paid,
                    native_fee_or_rebate,
                    native_qty_received,
                    order_id,
                    owner: _,
                    owner_slot: _,
                    fee_tier: _,
                    client_order_id: _,
                } => {
                    // Check already processed
                    if filled_order_ids.contains(&order_id) {
                        continue;
                    }

                    // Skip if not maker
                    if !maker {
                        continue;
                    }

                    let base_factor = token_factor(market.base_decimals);
                    let quote_factor = token_factor(market.quote_decimals);

                    let price_before_fees = Decimal::from(match side {
                        Side::Bid => native_qty_paid + native_fee_or_rebate,
                        Side::Ask => native_qty_received - native_fee_or_rebate,
                    });

                    let price = match side {
                        Side::Bid => {
                            (price_before_fees * base_factor)
                                / (quote_factor * Decimal::from(native_qty_received))
                        }
                        Side::Ask => {
                            (price_before_fees * base_factor)
                                / (quote_factor * Decimal::from(native_qty_paid))
                        }
                    };

                    let price_lots = price
                        .checked_mul(quote_factor)
                        .unwrap_or_default()
                        .checked_mul(Decimal::from(market.base_lot_size))
                        .unwrap_or_default()
                        .checked_div(base_factor)
                        .unwrap_or_default()
                        .checked_div(Decimal::from(market.quote_lot_size))
                        .unwrap_or_default()
                        .round();

                    let size = match side {
                        Side::Bid => Decimal::from(native_qty_received),
                        Side::Ask => Decimal::from(native_qty_paid),
                    }
                    .checked_div(base_factor)
                    .unwrap_or_default();
                    let size_lots = size
                        .checked_mul(quote_factor)
                        .unwrap_or_default()
                        .checked_div(Decimal::from(market.quote_lot_size))
                        .unwrap_or_default();

                    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

                    tracing::info!(
                        "OB fill: {} - {}, {}, {}",
                        market.name,
                        price,
                        size,
                        order_id
                    );

                    // Update filled order ids
                    filled_order_ids.insert(order_id);
                    trades_to_insert.push(MarketTrade {
                        slug: market.name.clone(),
                        order_id: Some(order_id.to_string()),
                        market_address: market.address.to_string(),
                        market_buy: match side {
                            Side::Ask => 0,
                            Side::Bid => 1,
                        },
                        avg_price: price,
                        amount: size,
                        index: 0,
                        timestamp: now,
                        blocktime: now,
                        avg_price_lots: price_lots,
                        amount_lots: size_lots,
                        slot: account.slot,
                        transaction_signature: account.txn_signature.clone(),
                    });
                }
                // Skip out event
                _ => {}
            }
        }

        // Insert trades into DB
        if trades_to_insert.len() > 0 {
            tokio::spawn({
                let supabase_clone = supabase_client.clone();
                let redis_clone = redis_client.clone();
                let url_clone = api_url.clone();

                async move {
                    let _ = update_trades(url_clone, redis_clone, supabase_clone, trades_to_insert)
                        .await;
                }
            });
        }
    } else {
        // Get ask/bid orders from account
        let is_bid = market.bids.eq(&account.pubkey);
        let data = Slab::new(&mut account.data);
        let leaves = data.traverse(is_bid);
        let levels = construct_levels(leaves, &market, 20);

        // Update local market state
        if is_bid {
            market_state.bids = levels;
        } else {
            market_state.asks = levels;
        }

        /*
        tracing::info!(
            "OB orders : {} - {} bids, {} asks",
            market.name,
            market_state.bids.len(),
            market_state.asks.len()
        );
        */

        // Publish ask/bid updates to redis
        publish_trades_data(&market.name, &market_state, redis_conn, account.slot)?;
    }

    Ok(())
}

/*
 * Function: parse_ob_markets
 * 1. Get account data using rpc client
 * 2. Parse market account and build market info account with market configuration
 */
pub async fn parse_ob_markets(
    rpc_client: &RpcClient,
    markets: Vec<MarketConfig>,
) -> anyhow::Result<Vec<ObMarketInfo>> {
    let rpc_config = RpcAccountInfoConfig {
        encoding: Some(UiAccountEncoding::Base64),
        data_slice: None,
        commitment: Some(CommitmentConfig::confirmed()),
        min_context_slot: None,
    };

    let market_keys = markets
        .iter()
        .filter(|x| x.ob_market_address.is_some())
        .map(|x| Pubkey::from_str(&x.ob_market_address.clone().unwrap()).unwrap())
        .collect::<Vec<Pubkey>>();
    let mut market_results = rpc_client
        .get_multiple_accounts_with_config(&market_keys, rpc_config.clone())
        .await?
        .value;

    let mut mint_key_map = HashMap::new();

    let mut market_infos = market_results
        .iter_mut()
        .map(|r| {
            let get_account_result = r.as_mut().unwrap();

            let mut market_bytes: &[u8] = &mut get_account_result.data[5..];
            let raw_market: ObMarketState =
                AnchorDeserialize::deserialize(&mut market_bytes).unwrap();

            let market_address = array_to_pubkey(raw_market.own_address);
            let bids_key = array_to_pubkey(raw_market.bids);
            let asks_key = array_to_pubkey(raw_market.asks);
            let event_queue_key = array_to_pubkey(raw_market.event_q);
            let base_mint_key = array_to_pubkey(raw_market.coin_mint);
            let quote_mint_key = array_to_pubkey(raw_market.pc_mint);
            mint_key_map.insert(base_mint_key, 0);
            mint_key_map.insert(quote_mint_key, 0);

            let market_name = markets
                .iter()
                .find(|x| {
                    x.ob_market_address.clone().unwrap_or_default() == market_address.to_string()
                })
                .unwrap()
                .slug
                .clone();

            ObMarketInfo {
                name: market_name,
                address: market_address,
                base_decimals: 0,
                quote_decimals: 0,
                base_mint: base_mint_key,
                quote_mint: quote_mint_key,
                bids: bids_key,
                asks: asks_key,
                event_queue: event_queue_key,
                base_lot_size: raw_market.coin_lot_size,
                quote_lot_size: raw_market.pc_lot_size,
            }
        })
        .collect::<Vec<ObMarketInfo>>();

    let mint_keys = mint_key_map.keys().cloned().collect::<Vec<Pubkey>>();

    let mint_results = rpc_client
        .get_multiple_accounts_with_config(&mint_keys, rpc_config)
        .await?
        .value;
    for i in 0..mint_results.len() {
        let mut mint_account = mint_results[i].as_ref().unwrap().clone();
        let mut mint_bytes: &[u8] = &mut mint_account.data[..];
        let mint = Mint::unpack_from_slice(&mut mint_bytes).unwrap();

        mint_key_map.insert(mint_keys[i], mint.decimals);
    }

    for i in 0..market_infos.len() {
        market_infos[i].base_decimals = *mint_key_map.get(&market_infos[i].base_mint).unwrap();
        market_infos[i].quote_decimals = *mint_key_map.get(&market_infos[i].quote_mint).unwrap();
    }

    Ok(market_infos)
}

/*
 * Function: parse_ob_orders
 * 1. Get account data using rpc client
 * 2. Parse order account and build levels as 20 limit
 */
pub async fn parse_ob_orders(
    rpc_client: &RpcClient,
    address: Pubkey,
    is_bid: bool,
    market: ObMarketInfo,
) -> anyhow::Result<Vec<MarketOrder>> {
    let rpc_config = RpcAccountInfoConfig {
        encoding: Some(UiAccountEncoding::Base64),
        data_slice: None,
        commitment: Some(CommitmentConfig::confirmed()),
        min_context_slot: None,
    };

    let mut account = rpc_client
        .get_account_with_config(&address, rpc_config.clone())
        .await?
        .value
        .unwrap();

    let data = Slab::new(&mut account.data);
    let leaves = data.traverse(is_bid);
    let orders = construct_levels(leaves, &market, 20);

    Ok(orders)
}
