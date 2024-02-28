use anyhow::Ok;
use num_traits::{FromPrimitive, ToPrimitive};
use postgrest::Postgrest;
use redis::{Client, Commands, Connection};
use sqlx::types::Decimal;
use std::{
    collections::HashMap,
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
    vec,
};

use anchor_lang::AnchorDeserialize;
use solana_account_decoder::UiAccountEncoding;
use solana_client::{nonblocking::rpc_client::RpcClient, rpc_config::RpcAccountInfoConfig};
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};

use crate::{
    constants::{
        BUY_LOG_PDA_SEED, CHANNEL_NAME, GD_ORDER_DEPTH, GIGADEX_PROGRAM_ID, SELL_LOG_PDA_SEED,
    },
    processor::market::{publish_trades_data, update_trades},
    structs::{
        geyser::Account,
        gigadex::{
            GdAsksData, GdBalance, GdBalanceData, GdBidsData, GdMarketInfo, GdMarketOrder,
            GdMarketOrderLog, GdMarketState, GdOrderData, OrderTree, UserBalances,
        },
        market::{MarketConfig, MarketOrder, MarketOrders, MarketTrade},
    },
    utils::{generate_publish_uid_data, token_factor},
};

/*
 * Function: parse_gigadex_account
 * 1. Parse account data from geyser subscribe
 * 2. If ask/bids account, then update orderbook data and publish compressed_orderbook
 * 3. If buy/sell account, then build trades data with price/amount calculation and call update_trades
 */
pub async fn parse_gigadex_account(
    api_url: String,
    redis_client: Client,
    supabase_client: Postgrest,
    market: GdMarketInfo,
    account: &mut Account,
    redis_conn: &mut Connection,
    market_orders: &mut HashMap<String, MarketOrders>,
    prev_uid_asks: &mut HashMap<String, HashMap<u64, Vec<GdMarketOrder>>>,
    prev_uid_bids: &mut HashMap<String, HashMap<u64, Vec<GdMarketOrder>>>,
    prev_balances: &mut HashMap<String, HashMap<u64, GdBalance>>,
) -> anyhow::Result<()> {
    let market_state = market_orders.get_mut(&market.address.to_string()).unwrap();
    let mut trades_to_insert: Vec<MarketTrade> = Vec::new();

    // Built account_info for parse data
    if market.asks.eq(&account.pubkey) || market.bids.eq(&account.pubkey) {
        let is_bid = market.bids.eq(&account.pubkey);

        let gd_orders = parse_order_account(&account.data)?;

        // Get previous uid orders for market
        let prev_uid_orders = (if is_bid {
            prev_uid_bids.get_mut(&market.name)
        } else {
            prev_uid_asks.get_mut(&market.name)
        })
        .unwrap();

        // Build current orders map
        let mut cur_orders: HashMap<u64, Vec<GdMarketOrder>> = HashMap::new();
        gd_orders.iter().for_each(|x| {
            let key = x.uid;
            if !cur_orders.contains_key(&key) {
                cur_orders.insert(key, vec![x.clone()]);
            } else {
                let orders = cur_orders.get_mut(&key).unwrap();
                orders.push(x.clone());
            }
        });

        // Refresh asks/bids data
        {
            let market_key = format!(
                "{}:{}",
                if is_bid { "uid_bids" } else { "uid_asks" },
                market.name
            );
            redis_conn.del(&market_key)?;

            let mut uid_orders = vec![];
            for (uid, orders) in cur_orders.iter() {
                // Compare with previous orders and publish event
                let orders_data = convert_orders_data(&orders, &market);
                let prev_orders = prev_uid_orders.get(uid);

                if prev_orders.is_some_and(|_orders| _orders != orders) || prev_orders.is_none() {
                    let msg =
                        build_order_data(is_bid, &market.name, *uid, &orders_data, account.slot);
                    redis_conn.publish(CHANNEL_NAME, msg)?;
                }

                let data = if is_bid {
                    serde_json::to_string(&GdBidsData {
                        uid_bids: orders_data,
                        slot: account.slot,
                    })
                } else {
                    serde_json::to_string(&GdAsksData {
                        uid_asks: orders_data,
                        slot: account.slot,
                    })
                }
                .unwrap_or_default();
                uid_orders.push((uid, data));
            }

            redis_conn.hset_multiple(&market_key, &uid_orders)?;
        }

        // Publish empty ask/bid updates
        {
            for (uid, _) in prev_uid_orders.into_iter() {
                // If uid not exists in cur_orders, then means ask/bid is empty
                if cur_orders.get(&uid).is_none() {
                    let msg = build_order_data(is_bid, &market.name, *uid, &vec![], account.slot);
                    redis_conn.publish(CHANNEL_NAME, msg)?;
                };
            }

            *prev_uid_orders = cur_orders.clone();
        }

        let orders = sort_orders(&gd_orders, &market, GD_ORDER_DEPTH, is_bid);

        // Update local market state
        if is_bid {
            market_state.bids = orders;
        } else {
            market_state.asks = orders;
        }

        // Publish ask/bid updates to redis
        publish_trades_data(&market.name, &market_state, redis_conn, account.slot)?;
    } else if market.buy_order_log.eq(&account.pubkey) || market.sell_order_log.eq(&account.pubkey)
    {
        let order: GdMarketOrderLog = AnchorDeserialize::deserialize(&mut &account.data[8..])?;
        if order.amount == 0 {
            return Ok(());
        }

        let price_lots = Decimal::from(order.total_value_lamports) / Decimal::from(order.amount);
        let amount_lots = order.amount;

        let price =
            price_lots_to_number(price_lots, market.base_decimals, market.quote_decimals, 0);
        let amount = base_lots_to_number(amount_lots, market.base_decimals);

        let market_buy = if market.buy_order_log.eq(&account.pubkey) {
            1
        } else {
            0
        };

        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        tracing::info!("GD fill: {} - {}, {}", market.name, price, amount,);

        trades_to_insert.push(MarketTrade {
            slug: market.name.clone(),
            market_address: market.address.to_string(),
            market_buy,
            avg_price: Decimal::from_f64(price).unwrap(),
            amount: Decimal::from_f64(amount).unwrap(),
            index: 0,
            timestamp: now,
            blocktime: now,
            avg_price_lots: price_lots,
            amount_lots: Decimal::from(amount_lots),
            slot: account.slot,
            transaction_signature: account.txn_signature.clone(),
            order_id: None,
        });
    } else if market.balances.eq(&account.pubkey) {
        let market_balances = parse_balances_account(&account.data, &market)?;

        // Refresh balances data
        {
            let balances_key = format!("balances:{}", market.name);
            redis_conn.del(&balances_key)?;

            let mut uid_balances = vec![];
            let prev_market_balances = prev_balances.get_mut(&market.name).unwrap();
            for (uid, balance) in market_balances.iter() {
                // Compare with previous balances and publish event
                let _prev_balance = prev_market_balances.get(uid);
                match _prev_balance {
                    Some(_balance) => {
                        if _balance != balance {
                            let msg = generate_publish_uid_data(
                                &market.name,
                                &GdBalanceData {
                                    claimable_balance: balance.clone(),
                                    slot: account.slot,
                                },
                                *uid,
                            );
                            redis_conn.publish(CHANNEL_NAME, msg)?;
                        }
                    }
                    None => {}
                };

                let data = serde_json::to_string(&GdBalanceData {
                    claimable_balance: balance.clone(),
                    slot: account.slot,
                })
                .unwrap_or_default();
                uid_balances.push((uid, data));
            }

            redis_conn.hset_multiple(&balances_key, &uid_balances)?;
            *prev_market_balances = market_balances;
        }
    }

    if trades_to_insert.len() > 0 {
        tokio::spawn({
            let supabase_clone = supabase_client.clone();
            let redis_clone = redis_client.clone();
            let url_clone = api_url.clone();

            async move {
                let _ =
                    update_trades(url_clone, redis_clone, supabase_clone, trades_to_insert).await;
            }
        });
    }

    Ok(())
}

/*
 * Function: parse_gd_markets
 * 1. Get account data using rpc client
 * 2. Parse market account and build market info account with market configuration
 */
pub async fn parse_gd_markets(
    rpc_client: &RpcClient,
    markets: &Vec<MarketConfig>,
) -> anyhow::Result<Vec<GdMarketInfo>> {
    let rpc_config = RpcAccountInfoConfig {
        encoding: Some(UiAccountEncoding::Base64),
        data_slice: None,
        commitment: Some(CommitmentConfig::confirmed()),
        min_context_slot: None,
    };

    let market_keys = markets
        .iter()
        .filter(|x| x.gd_market_address.is_some())
        .map(|x| Pubkey::from_str(&x.gd_market_address.clone().unwrap()).unwrap())
        .collect::<Vec<Pubkey>>();
    let mut market_results = rpc_client
        .get_multiple_accounts_with_config(&market_keys, rpc_config.clone())
        .await?
        .value;

    let gigadex_pubkey = Pubkey::from_str(GIGADEX_PROGRAM_ID)?;
    let market_infos: Vec<GdMarketInfo> = market_results
        .iter_mut()
        .enumerate()
        .map(|(idx, r)| {
            let get_account_result = r.as_mut().unwrap();

            let address = *market_keys.get(idx).unwrap();
            let mut market_bytes: &[u8] = &mut get_account_result.data[8..];
            let raw_market: GdMarketState =
                AnchorDeserialize::deserialize(&mut market_bytes).unwrap();
            let (buy_order_log, _) = Pubkey::find_program_address(
                &[&address.to_bytes(), BUY_LOG_PDA_SEED.as_bytes()],
                &gigadex_pubkey,
            );
            let (sell_order_log, _) = Pubkey::find_program_address(
                &[&address.to_bytes(), SELL_LOG_PDA_SEED.as_bytes()],
                &gigadex_pubkey,
            );

            let market_config = markets
                .iter()
                .find(|x| x.gd_market_address.clone().unwrap_or_default() == address.to_string())
                .unwrap();

            GdMarketInfo {
                address,
                name: market_config.slug.clone(),
                base_decimals: market_config.base_decimals,
                quote_decimals: market_config.quote_decimals,
                asks: raw_market.asks,
                bids: raw_market.bids,
                balances: raw_market.balances,
                buy_order_log,
                sell_order_log,
                multiplier: 1000000,
            }
        })
        .collect();

    Ok(market_infos)
}

/*
 * Function: parse_gd_orders
 * 1. Get account data using rpc client
 * 2. Parse order account and build levels as 20 limit
 */
pub async fn parse_gd_orders(
    rpc_client: &RpcClient,
    address: Pubkey,
) -> anyhow::Result<Vec<GdMarketOrder>> {
    let rpc_config = RpcAccountInfoConfig {
        encoding: Some(UiAccountEncoding::Base64),
        data_slice: None,
        commitment: Some(CommitmentConfig::confirmed()),
        min_context_slot: None,
    };

    let account = rpc_client
        .get_account_with_config(&address, rpc_config.clone())
        .await?
        .value
        .unwrap();

    let orders = parse_order_account(account.data.as_slice())?;
    Ok(orders)
}

/*
 * Function: parse_order_account
 * 1. Decode orderTree account and build orders data from nodes
 */
pub fn parse_order_account(data: &[u8]) -> anyhow::Result<Vec<GdMarketOrder>> {
    let order_tree = bytemuck::from_bytes::<OrderTree>(&data[8..]);
    let mut orders: Vec<GdMarketOrder> = Vec::new();

    for r in order_tree.nodes {
        if r.amount > 0 {
            orders.push(GdMarketOrder {
                uid: r.uid,
                price_lots: r.price,
                amount_lots: r.amount,
            });
        }
    }

    Ok(orders)
}

/*
 * Function: parse_balances_account
 * 1. Decode UserBalances account and build balances
 */
pub fn parse_balances_account(
    data: &[u8],
    market: &GdMarketInfo,
) -> anyhow::Result<HashMap<u64, GdBalance>> {
    let user_balances = bytemuck::from_bytes::<UserBalances>(&data[8..]);
    let mut balances: HashMap<u64, GdBalance> = HashMap::new();

    let max_users = user_balances.num_users + 1;
    for uid in 1..max_users {
        let r = user_balances.entries[uid as usize];
        balances.insert(
            uid,
            GdBalance {
                lamports: (Decimal::from(r.lamports) / token_factor(market.quote_decimals))
                    .to_f64()
                    .unwrap_or_default(),
                lots: base_lots_to_number(r.lots, market.base_decimals),
            },
        );
    }

    Ok(balances)
}

/*
 * Helper function for convert price_lots into readable price
 */
pub fn price_lots_to_number(
    lots: Decimal,
    base_decimals: u8,
    quote_decimals: u8,
    multiplier: u64,
) -> f64 {
    if multiplier > 0 {
        Decimal::to_f64(
            &(lots / Decimal::from(multiplier) * token_factor(base_decimals)
                / token_factor(quote_decimals)),
        )
        .unwrap_or_default()
    } else {
        Decimal::to_f64(&(lots * token_factor(base_decimals) / token_factor(quote_decimals)))
            .unwrap_or_default()
    }
}

/*
 * Helper function for convert amount_lots into readable amount
 */
pub fn base_lots_to_number(lots: u64, base_decimals: u8) -> f64 {
    Decimal::to_f64(&(Decimal::from(lots) / token_factor(base_decimals))).unwrap_or_default()
}

/*
 * Helper function for convert GdMarketOrders into MarketOrders in depth
 */
pub fn sort_orders(
    orders: &Vec<GdMarketOrder>,
    market: &GdMarketInfo,
    depth: usize,
    is_bid: bool,
) -> Vec<MarketOrder> {
    let mut orders_clone = orders.clone();
    orders_clone.sort_by_key(|x| x.price_lots);
    if is_bid {
        orders_clone.reverse();
    }

    let mut levels: Vec<(u64, u64)> = vec![];
    for x in orders_clone {
        let len = levels.len();
        if len > 0 && levels[len - 1].0 == x.price_lots {
            levels[len - 1].1 += x.amount_lots;
        } else if len == depth {
            break;
        } else {
            levels.push((x.price_lots, x.amount_lots));
        }
    }

    levels
        .into_iter()
        .map(|x| MarketOrder {
            price_lots: x.0,
            size_lots: x.1,
            price: price_lots_to_number(
                Decimal::from(x.0),
                market.base_decimals,
                market.quote_decimals,
                market.multiplier,
            ),
            amount: base_lots_to_number(x.1, market.base_decimals),
        })
        .collect()
}

/*
 * Helper function for convert GdMarketOrders into GdOrderData array
 */
pub fn convert_orders_data(orders: &Vec<GdMarketOrder>, market: &GdMarketInfo) -> Vec<GdOrderData> {
    orders
        .into_iter()
        .map(|x| GdOrderData {
            price_lots: x.price_lots,
            amount_lots: x.amount_lots,
            price: price_lots_to_number(
                Decimal::from(x.price_lots),
                market.base_decimals,
                market.quote_decimals,
                market.multiplier,
            ),
            amount: base_lots_to_number(x.amount_lots, market.base_decimals),
        })
        .collect()
}

pub fn build_order_data(
    is_bid: bool,
    market: &String,
    uid: u64,
    orders: &Vec<GdOrderData>,
    slot: u64,
) -> String {
    if is_bid {
        generate_publish_uid_data(
            &market,
            &GdBidsData {
                uid_bids: orders.to_vec(),
                slot: slot,
            },
            uid,
        )
    } else {
        generate_publish_uid_data(
            &market,
            &GdAsksData {
                uid_asks: orders.to_vec(),
                slot: slot,
            },
            uid,
        )
    }
}
