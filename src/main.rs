mod api;
mod constants;
mod parser;
mod processor;
mod structs;
mod utils;

use postgrest::Postgrest;
use solana_client::nonblocking::rpc_client;
use solana_sdk::commitment_config::CommitmentConfig;
use dotenv::dotenv;
use std::{env, time::Duration};
use tokio::{time::sleep, try_join};
use yellowstone_grpc_client::GeyserGrpcClient;
use crate::processor::*;

use crate::structs::*;

#[tokio::main]
async fn main() {

    // Environment configuration
    dotenv().ok();    
    let api_url = env::var("API_URL").expect("API_URL not set in .env");
    let redis_url = env::var("REDIS_URL").expect("REDIS_URL not set in .env");
    let rpc_url = env::var("RPC_URL").expect("RPC_URL not set in .env");
    let supabase_url = env::var("SUPABASE_URL").expect("SUPABASE_URL not set in .env");
    let supabase_auth_token =
    env::var("SUPABASE_AUTH_TOKEN").expect("SUPABASE_AUTH_TOKEN not set in .env");
    let triton_url = env::var("TRITON_URL").expect("TRITON_URL not set in .env");
    let triton_token = env::var("TRITON_TOKEN").expect("TRITON_TOKEN not set in .env");

    let anchor_account_address = "5BUwFW4nRbftYTDMbgxykoFWqWHPzahFSNAaaaJtVKsq";

    // Tracing configuration
    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_max_level(tracing::Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("failed to set global tracing sub");
    tracing::info!("Initializing server v11");

    // Connect redis
    let redis_client = redis::Client::open(redis_url.clone()).expect("Failed to connect to redis");

    // Connect supabase
    let supabase_client =
        Postgrest::new(supabase_url).insert_header("apikey", supabase_auth_token.clone());

    // Connect geyser client
    let mut geyser_client = GeyserGrpcClient::connect_with_timeout(
        triton_url,
        Some(triton_token),
        None,
        Some(Duration::from_secs(10)),
        Some(Duration::from_secs(10)),
        false,
    )
    .await
    .map_err(|e| tracing::error!("Geyser error: {:?}", e))
    .expect("failed to connect geyser");
    tracing::info!("Connected to geyser...");

    // Subscribe openbook & gigadex events
    let subscribe_task = tokio::spawn({
        let rpc_client =
            rpc_client::RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());

        async move {
            loop {
                let ret = subscribe_geyser(
                    api_url.clone(),
                    &redis_client,
                    &supabase_client,
                    &rpc_client,
                    &mut geyser_client,
                )
                .await;

                if ret.is_err() {
                    tracing::error!("Subscribe error: {:?}", ret.err());
                };
            }
        }
    });

    let health_check_task = tokio::spawn({
        async move {
            loop {
                sleep(Duration::from_secs(60)).await;
                tracing::info!("--- Live ---");
            }
        }
    });

    // Wait for join tasks
    try_join!(subscribe_task, health_check_task,).expect("Error to finish task");

    //jack-dev new plugin output 1
    let ( price, amount, is_buy ) = extractor(
        api_url: api_url, 
        redis_client: redis_client, 
        market_address: String
    );
    println!("price: {}, amount: {}, is_buy: {}", price, amount, is_buy);

    //jack-dev new plugin output 2
    if let Some(aum_usd_value) = get_aum_usd_data(anchor_account_address) {
        println!("aum_usd: {}", aum_usd_value);
    } else {
        println!("Error retrieving aum_usd data.");
    }

}
