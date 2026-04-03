use gettextrs::gettext;
use rand::prelude::IndexedRandom;
use reqwest::Client;
use serde_json::{json, Value};
use std::error::Error;
use std::time::SystemTime;
use uuid::Uuid;

use crate::core::fingerprinting::signature_format::DecodedSignature;
use crate::core::fingerprinting::user_agent::USER_AGENTS;

pub async fn recognize_song_from_signature(
    client: &Client,
    signature: &DecodedSignature,
) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let user_agent = USER_AGENTS.choose(&mut rand::rng()).unwrap();

    let timestamp_ms = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_millis();

    let post_data = json!({
        "geolocation": {
            "altitude": 300,
            "latitude": 45,
            "longitude": 2
        },
        "signature": {
            "samplems": (signature.number_samples as f32 / signature.sample_rate_hz as f32 * 1000.) as u32,
            "timestamp": timestamp_ms as u32,
            "uri": signature.encode_to_uri().map_err(|e| e.to_string())?
        },
        "timestamp": timestamp_ms as u32,
        "timezone": "Europe/Paris"
    })
    .to_string();

    let uuid_1 = Uuid::new_v4().hyphenated().to_string().to_uppercase();
    let uuid_2 = Uuid::new_v4().hyphenated().to_string();

    let url = format!(
        "https://amp.shazam.com/discovery/v5/en/US/android/-/tag/{}/{}\
?sync=true\
&webv3=true\
&sampling=true\
&connected=\
&shazamapiversion=v3\
&sharehub=true\
&video=v3",
        uuid_1, uuid_2
    );

    let response = client
        .post(&url)
        .header("Content-Language", "en_US")
        .header("Content-Type", "application/json")
        .header("User-Agent", *user_agent)
        .body(post_data)
        .send()
        .await?;

    if response.status().as_u16() == 429 {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::QuotaExceeded,
            gettext("Your IP has been rate-limited").as_str(),
        )));
    }

    let bytes = response.bytes().await?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub async fn obtain_raw_cover_image(
    client: &Client,
    url: &str,
) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
    let user_agent = USER_AGENTS.choose(&mut rand::rng()).unwrap();

    let response = client
        .get(url)
        .header("Content-Language", "en_US")
        .header("User-Agent", *user_agent)
        .send()
        .await?;

    Ok(response.bytes().await?.to_vec())
}
