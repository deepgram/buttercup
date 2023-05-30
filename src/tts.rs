use crate::audio;
use crate::state::State;
use base64::{engine::general_purpose, Engine};
use reqwest::multipart::Form;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Deserialize, Debug)]
pub struct TtsResponse {
    results: Vec<TtsResults>,
}

#[derive(Deserialize, Debug)]
pub struct TtsResults {
    audio: Vec<f32>,
}

#[derive(Serialize, Debug)]
pub struct TtsTexts(Vec<String>);

pub fn form_from_string(input: String) -> Form {
    let body = TtsTexts(vec![input]);
    let body = serde_json::to_string(&body).expect("could not create String from TtsTexts");
    let json_part = reqwest::multipart::Part::text(body)
        .file_name("texts")
        .mime_str("application/json")
        .expect("mime string should parse");
    reqwest::multipart::Form::new().part("texts", json_part)
}

// returns the speech as base64 encoded 8000 Hz mulaw audio (i.e. a String!)
pub async fn text_to_speech(input: String, state: Arc<State>) -> String {
    let client = reqwest::Client::new();
    let tts_response: TtsResponse = client
        .post("https://api.sandbox.deepgram.com/nlu?synthesize=true&speed=1.0&pitch_steps=2&variability=0.5")
        .header("Authorization", format!("Token {}", state.deepgram_api_key))
        .multipart(form_from_string(input))
        .send()
        .await
        .expect("unable to make tts request")
        .json()
        .await
        .expect("unable to parse tts request");

    let f32_samples = tts_response.results[0].audio.clone();

    // take every other sample since the sample rate is 16000...
    let mut i16_samples = Vec::new();
    for i in 0..(f32_samples.len() / 2) {
        let i16_sample = audio::f32_to_i16(f32_samples[i * 2]);
        i16_samples.push(i16_sample);
    }

    let mut mulaw_samples = Vec::new();
    for sample in i16_samples {
        mulaw_samples.push(audio::linear_to_ulaw(sample));
    }

    // base64 encode the mulaw
    general_purpose::STANDARD.encode(&mulaw_samples)
}
