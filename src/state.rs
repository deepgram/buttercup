use axum::extract::ws::WebSocket;
use futures::lock::Mutex;

pub struct State {
    pub deepgram_url: String,
    pub deepgram_api_key: String,
    pub subscribers: Mutex<Vec<WebSocket>>,
    pub pre_call_prompt: Mutex<String>,
    pub initial_call_message: Mutex<String>,
    pub post_call_prompts: Mutex<Vec<String>>,
}
