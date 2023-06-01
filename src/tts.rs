use crate::audio;
use crate::state;
use aws_sdk_polly::model::{OutputFormat, VoiceId};
use base64::{engine::general_purpose, Engine};
use reqwest::multipart::Form;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub async fn tts(input: String, state: Arc<state::State>) -> String {
    println!("{:?} making TTS request", chrono::Utc::now());
    let result = match &state.tts {
        state::Tts::Deepgram(deepgram_api_key) => tts_dg(input, deepgram_api_key.clone()).await,
        state::Tts::Aws => tts_aws(input).await,
    };
    println!("{:?} finished TTS request", chrono::Utc::now());

    result
}

pub async fn tts_aws(input: String) -> String {
    let shared_config = aws_config::from_env().load().await;
    let client = aws_sdk_polly::Client::new(&shared_config);

    let polly_response = client
        .synthesize_speech()
        .output_format(OutputFormat::Pcm)
        .sample_rate("8000")
        .text(input)
        .voice_id(VoiceId::Joanna)
        .send()
        .await
        .expect("unable to get Polly response");

    let pcm = polly_response
        .audio_stream
        .collect()
        .await
        .map(|aggregated_bytes| aggregated_bytes.to_vec())
        .expect("unable to get PCM from Polly response");

    let mut i16_samples = Vec::new();
    for i in 0..(pcm.len() / 2) {
        let mut i16_sample = pcm[i * 2] as i16;
        i16_sample |= ((pcm[i * 2 + 1]) as i16) << 8;
        i16_samples.push(i16_sample);
    }

    let mut mulaw_samples = Vec::new();
    for sample in i16_samples {
        mulaw_samples.push(audio::linear_to_ulaw(sample));
    }
    general_purpose::STANDARD.encode(&mulaw_samples)
}

#[derive(Deserialize, Debug)]
pub struct DgTtsResponse {
    results: Vec<DgTtsResults>,
}

#[derive(Deserialize, Debug)]
pub struct DgTtsResults {
    audio: Vec<f32>,
}

#[derive(Serialize, Debug)]
pub struct DgTtsTexts(Vec<String>);

pub fn form_from_string(input: String) -> Form {
    let body = DgTtsTexts(vec![input]);
    let body = serde_json::to_string(&body).expect("could not create String from TtsTexts");
    let json_part = reqwest::multipart::Part::text(body)
        .file_name("texts")
        .mime_str("application/json")
        .expect("mime string should parse");
    reqwest::multipart::Form::new().part("texts", json_part)
}

// returns the speech as base64 encoded 8000 Hz mulaw audio (i.e. a String!)
pub async fn tts_dg(input: String, deepgram_api_key: String) -> String {
    let client = reqwest::Client::new();
    let tts_response: DgTtsResponse = client
        .post("https://api.sandbox.deepgram.com/nlu?synthesize=true&speed=1.0&pitch_steps=2&variability=0.5")
        .header("Authorization", format!("Token {}", deepgram_api_key))
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
