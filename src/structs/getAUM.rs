
use solana_client::{
    client::Client,
    rpc_client::RpcClient
};

use std::{
    collections::{HashMap, HashSet},
    error::Error,
    str::FromStr,
    time::Duration,
    vec,
};

use solana_program::pubkey::Pubkey;

pub async fn get_aum_usd_data(anchor_account_address: &str) -> Option<u128> {

    let rpc_client = RpcClient::new("https://api.mainnet-beta.solana.com".to_string());
    let account_pubkey = Pubkey::from_str(account_address).unwrap();
    let account_data = rpc_client.get_account_data(&account_pubkey).unwrap();
    let data_slice = &account_data.data[..16];
    let aum_usd: u128 = u128::from_le_bytes(data_slice.try_into().unwrap());
    Some(aum_usd)

}

