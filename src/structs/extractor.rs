
use openbook_dex::{
    matching::Side,
    state::{
        strip_header, 
        Event,
        EventQueueHeader, 
        EventView, 
        Queue
    },
};
use redis::{
    Client, 
    Connection
};
use sqlx::types::Decimal;
use std::{
    collections::{HashMap, HashSet},
    error::Error,
    str::FromStr,
    time::Duration,
    vec,
};

use anchor_lang::AnchorDeserialize;
use solana_account_decoder::UiAccountEncoding;
use solana_client::{
    nonblocking::rpc_client::RpcClient, 
    rpc_config::RpcAccountInfoConfig
};
use solana_sdk::{
    account_info::AccountInfo, 
    commitment_config::CommitmentConfig, 
    program_pack::Pack, pubkey::Pubkey
};

use crate::{
    processor::market::{
        publish_trades_data, 
        update_trades
    },
    structs::{
        geyser::Account,
        market::{MarketConfig, MarketOrder},
        mint::Mint,
        openbook::{ObMarketInfo, ObMarketState},
        slab::{construct_levels, Slab},
    },
    utils::{array_to_pubkey, token_factor},
};

use yellowstone_grpc_client::{
    GeyserGrpcClient, 
    GeyserGrpcClientError
};
use yellowstone_grpc_proto::{
    prelude::{
        subscribe_update::UpdateOneof, 
        SubscribeRequest,
        SubscribeRequestFilterAccounts,
    },
    tonic::service::Interceptor,
};


pub async fn extractor (
    api_url: String,
    redis_client: &Client,
    rpc_client: &RpcClient,
    market_address: String,
) -> Result<(), Box<dyn Error>> {

    tracing::info!("Extracting data...");

    let market_state = market_orders
      .get_mut(&market.address.to_string())
      .unwrap();

    let mut api_url_conn = api_url
      .get_connection()
      .expect("Failed to get api url connection");

    let mut redis_conn = redis_client
     .get_connection()
     .expect("Failed to get redis connection");

     let mut rcp_client_conn = rcp_client
     .get_connection()
     .expect("Failed to get rcp client connection");

     .get_connection()
     .expect("Failed to get geyser client connection");

     let mut api_url_conn = api_url_conn.clone();
     let mut redis_conn = redis_conn.clone();
     let mut rcp_client_conn = rcp_client_conn.clone();

     let mut accounts: Vec<String> = Vec::new();
     let market_keys: Vec<String> = redis_conn
      .smembers("markets")
      .expect("Get markets failed");

    let mut markets: Vec<MarketConfig> = Vec::new();

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
        })
        .collect::<Vec<ObMarketInfo>>();

    if market_infos .is_empty() {
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

        let amount = match side {
            Side::Bid => Decimal::from(native_qty_received),
            Side::Ask => Decimal::from(native_qty_paid),
        }
        .checked_div(base_factor)
        .unwrap_or_default();

        let is_buy = match side {
            Side::Bid => true,
            Side::Ask => false,
        }

        tracing::info!(
            "OpenBook market fills: {}, {}, {}",
            price,
            amount,
            is_buy
        );
        let output = (
            price,
            amount,
            is_buy
        );
        OK(output)
    }

}


