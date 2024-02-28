use futures::{sink::SinkExt, stream::StreamExt};
use postgrest::Postgrest;
use redis::{Client, Commands};
use solana_client::nonblocking::rpc_client::RpcClient;
use std::{
    collections::{HashMap, HashSet},
    error::Error,
    time::Duration,
    vec,
};
use tokio::time::sleep;
use yellowstone_grpc_client::{GeyserGrpcClient, GeyserGrpcClientError};
use yellowstone_grpc_proto::{
    prelude::{
        subscribe_update::UpdateOneof, CommitmentLevel, SubscribeRequest,
        SubscribeRequestFilterAccounts,
    },
    tonic::service::Interceptor,
};

use crate::{
    constants::{DELAY_MILISEC, GD_ORDER_DEPTH, GIGADEX_PROGRAM_ID, OPENBOOK_PROGRAM_ID},
    parser::{
        parse_gd_markets, parse_gd_orders, parse_gigadex_account, parse_ob_markets,
        parse_ob_orders, parse_openbook_account, sort_orders,
    },
    processor::market::publish_trades_data,
    structs::{
        geyser::Account,
        gigadex::{GdBalance, GdMarketOrder},
        market::{MarketConfig, MarketOrders},
    },
};

type AccountsFilterMap = HashMap<String, SubscribeRequestFilterAccounts>;

/*
 * Function: subscribe_geyser
 * 1. Get active markets from redis as markets key
 * 2. Parse openbook and gigadex's ask/bid/fill accounts
 * 3. Build accounts list for subscribe and initial market states
 * 4. Loop subscribe geyser and if account update matched, process account update using OB or GD parser
 */
pub async fn subscribe_geyser(
    api_url: String,
    redis_client: &Client,
    supabase_client: &Postgrest,
    rpc_client: &RpcClient,
    geyser_client: &mut GeyserGrpcClient<impl Interceptor>,
) -> Result<(), Box<dyn Error>> {
    tracing::info!("Subscribe geyser...");
    let mut redis_conn = redis_client
        .get_connection()
        .expect("Failed to get redis connection");

    let mut accounts: Vec<String> = Vec::new();

    // Load markets
    let market_keys: Vec<String> = redis_conn.smembers("markets").expect("Get markets failed");

    let mut markets: Vec<MarketConfig> = Vec::new();
    for market in market_keys {
        let market_info: HashMap<String, String> =
            redis_conn.hgetall(format!("market_info:{market}"))?;
        if !market_info.contains_key("name") {
            continue;
        }

        let base_decimals = market_info.get("base_decimals").unwrap();
        let quote_decimals = market_info.get("quote_decimals").unwrap();

        markets.push(MarketConfig {
            gd_market_address: market_info.get("gd_market_address").cloned(),
            ob_market_address: market_info.get("ob_market_address").cloned(),
            name: market_info.get("name").unwrap().to_string(),
            slug: market_info.get("slug").unwrap().to_string(),
            status: market_info.get("status").unwrap().to_string(),
            base_decimals: u8::from_str_radix(&base_decimals, 10)?,
            quote_decimals: u8::from_str_radix(&quote_decimals, 10)?,
        });
    }

    let mut market_orders: HashMap<String, MarketOrders> = HashMap::new();

    // Prepare openbook accounts
    let mut ob_order_ids: HashSet<u128> = HashSet::new();
    let ob_markets = parse_ob_markets(rpc_client, markets.clone())
        .await
        .expect("Load openbook markets failed");

    for market in ob_markets.clone() {
        let market_key = market.address.clone();
        let asks_key = market.asks.clone();
        let bids_key = market.bids.clone();
        accounts.push(asks_key.to_string());
        accounts.push(bids_key.to_string());
        accounts.push(market.event_queue.to_string());

        let asks = match parse_ob_orders(rpc_client, asks_key, false, market.clone()).await {
            Ok(asks) => asks,
            Err(_) => Vec::new(),
        };
        let bids = match parse_ob_orders(rpc_client, bids_key, true, market.clone()).await {
            Ok(asks) => asks,
            Err(_) => Vec::new(),
        };

        // Publish initial orderbook data
        let market_order = MarketOrders { asks, bids };
        publish_trades_data(&market.name, &market_order, &mut redis_conn, 0)?;

        market_orders.insert(market_key.to_string(), market_order);
    }

    // Prepare gigadex accounts
    let mut gd_balances: HashMap<String, HashMap<u64, GdBalance>> = HashMap::new();
    let mut gd_uid_asks: HashMap<String, HashMap<u64, Vec<GdMarketOrder>>> = HashMap::new();
    let mut gd_uid_bids: HashMap<String, HashMap<u64, Vec<GdMarketOrder>>> = HashMap::new();
    let gd_markets = parse_gd_markets(rpc_client, &markets)
        .await
        .expect("Load gigadex markets failed");
    for market in gd_markets.iter() {
        let market_key = market.address.clone();
        accounts.push(market.asks.to_string());
        accounts.push(market.bids.to_string());
        accounts.push(market.balances.to_string());
        accounts.push(market.buy_order_log.to_string());
        accounts.push(market.sell_order_log.to_string());

        let asks = parse_gd_orders(rpc_client, market.asks).await?;
        let bids = parse_gd_orders(rpc_client, market.bids).await?;

        // Build initial orderbook data
        let market_order = MarketOrders {
            asks: sort_orders(&asks, market, GD_ORDER_DEPTH, false),
            bids: sort_orders(&bids, market, GD_ORDER_DEPTH, true),
        };
        publish_trades_data(&market.name, &market_order, &mut redis_conn, 0)?;

        // Build initial uid orders
        gd_uid_asks.insert(market.name.clone(), HashMap::new());
        let uid_asks = gd_uid_asks.get_mut(&market.name).unwrap();
        asks.iter().for_each(|x| {
            let uid = x.uid;
            if !uid_asks.contains_key(&uid) {
                uid_asks.insert(uid, vec![]);
            }
            let orders = uid_asks.get_mut(&uid).unwrap();
            orders.push(x.clone());
        });

        gd_uid_bids.insert(market.name.clone(), HashMap::new());
        let uid_bids = gd_uid_bids.get_mut(&market.name).unwrap();
        bids.iter().for_each(|x| {
            let uid = x.uid;
            if !uid_bids.contains_key(&uid) {
                uid_bids.insert(uid, vec![]);
            }
            let orders = uid_bids.get_mut(&uid).unwrap();
            orders.push(x.clone());
        });

        // Build initial balances data
        gd_balances.insert(market.name.clone(), HashMap::new());

        market_orders.insert(market_key.to_string(), market_order);
    }

    // Prepare geyser client
    let mut request = SubscribeRequest::default();
    request.set_commitment(CommitmentLevel::Confirmed);
    let mut accounts_filter: AccountsFilterMap = HashMap::new();
    accounts_filter.insert(
        "client".to_string(),
        SubscribeRequestFilterAccounts {
            account: accounts,
            owner: [
                OPENBOOK_PROGRAM_ID.to_string(),
                GIGADEX_PROGRAM_ID.to_string(),
            ]
            .into(),
            filters: [].into(),
        },
    );
    request.accounts = accounts_filter;

    // Subscribe geyser events
    loop {
        let (mut subscribe_tx, mut stream) = geyser_client.subscribe().await?;
        subscribe_tx
            .send(request.clone())
            .await
            .map_err(GeyserGrpcClientError::SubscribeSendError)?;
        tracing::info!("{} markets subscribed", markets.len());

        while let Some(message) = stream.next().await {
            match message {
                Ok(msg) => {
                    #[allow(clippy::single_match)]
                    #[allow(clippy::multiple_unsafe_ops_per_block)]
                    match msg.update_oneof {
                        Some(UpdateOneof::Account(account)) => {
                            let mut account: Account = account.into();
                            let account_address = account.pubkey;

                            // Parse OB market order
                            let ob_market = ob_markets
                                .iter()
                                .find(|x| x.is_valid_account(&account_address));
                            if ob_market.is_some() {
                                let market = ob_market.unwrap();
                                let _ = parse_openbook_account(
                                    api_url.clone(),
                                    redis_client.clone(),
                                    supabase_client.clone(),
                                    market.clone(),
                                    &mut account,
                                    &mut redis_conn,
                                    &mut market_orders,
                                    &mut ob_order_ids,
                                )
                                .await;
                            };

                            // Parse GD market order
                            let gd_market = gd_markets
                                .iter()
                                .find(|x| x.is_valid_account(&account_address));
                            if gd_market.is_some() {
                                let market = gd_market.unwrap();
                                let _ = parse_gigadex_account(
                                    api_url.clone(),
                                    redis_client.clone(),
                                    supabase_client.clone(),
                                    market.clone(),
                                    &mut account,
                                    &mut redis_conn,
                                    &mut market_orders,
                                    &mut gd_uid_asks,
                                    &mut gd_uid_bids,
                                    &mut gd_balances,
                                )
                                .await;
                            }
                        }
                        _ => {}
                    }
                }
                Err(e) => {
                    tracing::error!("Error geyser streaming: {:?}", e);
                    sleep(Duration::from_millis(DELAY_MILISEC)).await;
                }
            }
        }
    }
}
