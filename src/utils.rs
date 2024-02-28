use num_traits::FromPrimitive;
use solana_sdk::pubkey::Pubkey;
use sqlx::types::Decimal;

use crate::structs::market::{PublishAllData, PublishUidData};

pub fn generate_publish_data<F>(market: &str, data: &F, id: Option<String>) -> String
where
    F: serde::Serialize + Clone,
{
    let publish_data: PublishAllData<F> = PublishAllData {
        _type: "general".to_string(),
        market: market.to_string(),
        data: data.clone(),
        id,
    };

    let json_string = serde_json::to_string(&publish_data).expect("Failed to serialize to JSON");
    json_string
}

pub fn generate_publish_uid_data<F>(market: &str, data: &F, uid: u64) -> String
where
    F: serde::Serialize + Clone,
{
    let publish_data: PublishUidData<F> = PublishUidData {
        _type: uid,
        market: market.to_string(),
        data: data.clone(),
    };

    let json_string = serde_json::to_string(&publish_data).expect("Failed to serialize to JSON");
    json_string
}

pub fn token_factor(decimals: u8) -> Decimal {
    Decimal::from_u64(10u64.pow(decimals as u32)).unwrap()
}

pub fn array_to_pubkey(data: [u64; 4]) -> Pubkey {
    Pubkey::new_from_array(
        data.iter()
            .flat_map(|&x| x.to_le_bytes())
            .collect::<Vec<_>>()
            .try_into()
            .unwrap_or_else(|_| [0; 32]),
    )
}
