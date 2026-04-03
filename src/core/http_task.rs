use gettextrs::gettext;
use regex::Regex;
use reqwest::Client;
use serde_json::{to_string_pretty, Value};
use std::error::Error;

use crate::core::thread_messages::*;

use crate::core::fingerprinting::communication::{
    obtain_raw_cover_image, recognize_song_from_signature,
};
use crate::core::fingerprinting::signature_format::DecodedSignature;

async fn try_recognize_song(
    client: &Client,
    signature: DecodedSignature,
) -> Result<SongRecognizedMessage, Box<dyn Error + Send + Sync>> {
    let json_object = recognize_song_from_signature(client, &signature).await?;

    let mut album_name: Option<String> = None;
    let mut release_year: Option<String> = None;

    if let Value::Array(sections) = &json_object["track"]["sections"] {
        for section in sections {
            if let Value::String(string) = &section["type"] {
                if string == "SONG" {
                    if let Value::Array(metadata) = &section["metadata"] {
                        for metadatum in metadata {
                            if let Value::String(title) = &metadatum["title"] {
                                if title == "Album" {
                                    if let Value::String(text) = &metadatum["text"] {
                                        album_name = Some(text.to_string());
                                    }
                                } else if title == "Released" {
                                    if let Value::String(text) = &metadatum["text"] {
                                        release_year = Some(text.to_string());
                                    }
                                }
                            }
                        }
                        break;
                    }
                }
            }
        }
    }

    Ok(SongRecognizedMessage {
        artist_name: match &json_object["track"]["subtitle"] {
            Value::String(string) => string.to_string(),
            _ => {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    gettext("No match for this song").as_str(),
                )))
            }
        },
        album_name: album_name,
        song_name: match &json_object["track"]["title"] {
            Value::String(string) => string.to_string(),
            _ => {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    gettext("No match for this song").as_str(),
                )))
            }
        },
        cover_image: match &json_object["track"]["images"]["coverart"] {
            Value::String(string) => {
                Some(obtain_raw_cover_image(client, string).await?)
            }
            _ => None,
        },
        track_key: match &json_object["track"]["key"] {
            Value::String(string) => string.to_string(),
            _ => {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    gettext("No match for this song").as_str(),
                )))
            }
        },
        release_year: release_year,
        genre: match &json_object["track"]["genres"]["primary"] {
            Value::String(string) => Some(string.to_string()),
            _ => None,
        },
        shazam_json: Regex::new("\n *")
            .unwrap()
            .replace_all(
                &Regex::new("([,:])\n *")
                    .unwrap()
                    .replace_all(&to_string_pretty(&json_object).unwrap(), "$1 ")
                    .into_owned(),
                "",
            )
            .into_owned(),
    })
}

pub async fn http_task(
    http_rx: async_channel::Receiver<HTTPMessage>,
    gui_tx: async_channel::Sender<GUIMessage>,
    microphone_tx: async_channel::Sender<MicrophoneMessage>,
) {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .expect("Failed to build reqwest client");

    while let Ok(message) = http_rx.recv().await {
        match message {
            HTTPMessage::RecognizeSignature(signature) => {
                match try_recognize_song(&client, *signature).await {
                    Ok(recognized_song) => {
                        gui_tx
                            .try_send(GUIMessage::SongRecognized(Box::new(recognized_song)))
                            .unwrap();
                        gui_tx.try_send(GUIMessage::NetworkStatus(true)).unwrap();
                        gui_tx.try_send(GUIMessage::RateLimitState(false)).unwrap();
                    }
                    Err(error) => match error.to_string().as_str() {
                        a if a == gettext("No match for this song") => {
                            gui_tx
                                .try_send(GUIMessage::ErrorMessage(error.to_string()))
                                .unwrap();
                            gui_tx.try_send(GUIMessage::NetworkStatus(true)).unwrap();
                            gui_tx.try_send(GUIMessage::RateLimitState(false)).unwrap();
                        }
                        a if a == gettext("Your IP has been rate-limited") => {
                            gui_tx.try_send(GUIMessage::RateLimitState(true)).unwrap();
                        }
                        _ => {
                            log::error!("Network reach error: {:?}", error);
                            gui_tx.try_send(GUIMessage::NetworkStatus(false)).unwrap();
                        }
                    },
                };

                microphone_tx
                    .try_send(MicrophoneMessage::ProcessingDone)
                    .unwrap();
            }
        }
    }
}
