use axum::extract::ws::WebSocket;
use futures::lock::Mutex;

pub enum Llm {
    Deepgram,
    ChatGPT(String),
}

pub enum Tts {
    Deepgram(String),
    Aws,
}

pub struct State {
    pub deepgram_url: String,
    pub deepgram_api_key: String,
    pub llm: Llm,
    pub tts: Tts,
    pub subscribers: Mutex<Vec<WebSocket>>,
    pub pre_call_prompt: Mutex<String>,
    pub initial_call_message: Mutex<String>,
    pub introspection_prompt: Mutex<String>,
    pub introspection_carry_on: Mutex<Option<String>>,
    pub post_call_prompts: Mutex<Vec<String>>,
}
