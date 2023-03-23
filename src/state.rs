use axum::extract::ws::WebSocket;
use futures::lock::Mutex;
use std::collections::HashMap;

pub struct State {
    pub deepgram_url: String,
    pub deepgram_api_key: String,
    pub chatgpt_api_key: String,
    pub subscribers: Mutex<HashMap<String, Vec<WebSocket>>>,
}
