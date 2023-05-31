use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LlmMessage {
    pub role: String,
    pub content: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LlmRequest {
    pub text: Vec<LlmMessage>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LlmResponse {
    pub results: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WrappedLlmResponse {
    pub stream_sid: String,
    pub post_call: bool,
    pub llm_response: LlmResponse,
}
