use std::error::Error;

use crate::structs::market::*;

pub async fn get_summary(api_url: &String, market: &String) -> Result<SummaryData, Box<dyn Error>> {
    let endpoint_url = format!("{}{}/{}", api_url, "v2/summary", market);
    let response = match reqwest::get(endpoint_url).await {
        Ok(data) => data,
        Err(e) => {
            tracing::error!("Error call get_summary: {}", e);
            return Err(Box::new(e));
        }
    };

    let data: String = response.text().await?;

    // Parse JSON string
    let result = match serde_json::from_str::<SummaryResponse>(&data) {
        Ok(payload_json) => payload_json,
        Err(e) => {
            tracing::error!("Error parsing summary response: {}", e);
            return Err(Box::new(e));
        }
    };

    Ok(result.message)
}


