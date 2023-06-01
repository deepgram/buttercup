use crate::state;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LlmMessage {
    pub role: String,
    pub content: String,
}

pub async fn llm(llm_messages: Vec<LlmMessage>, state: Arc<state::State>) -> String {
    println!("{:?} making LLM request", chrono::Utc::now());
    let result = match &state.llm {
        state::Llm::Deepgram => llm_dg(llm_messages).await,
        state::Llm::ChatGPT(chatgpt_api_key) => {
            llm_chatgpt(llm_messages, chatgpt_api_key.clone()).await
        }
    };
    println!("{:?} finished LLM request", chrono::Utc::now());

    result
}

pub fn llm_messages_to_string(llm_messages: Vec<LlmMessage>) -> String {
    let mut result = String::new();
    for llm_message in llm_messages {
        // TODO: this role assignment is opinionated/not configurable
        let role = if llm_message.role == "user" {
            "Customer"
        } else {
            "Me"
        };
        result = format!("{}: {} ", role, llm_message.content).to_string();
    }
    result
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatGPTRequest {
    model: String,
    messages: Vec<LlmMessage>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatGPTResponse {
    id: String,
    object: String,
    created: u64,
    choices: Vec<ChatGPTChoice>,
    usage: ChatGPTUsage,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatGPTUsage {
    prompt_tokens: usize,
    completion_tokens: usize,
    total_tokens: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatGPTChoice {
    index: usize,
    message: LlmMessage,
    finish_reason: String,
}

pub async fn llm_chatgpt(llm_messages: Vec<LlmMessage>, chatgpt_api_key: String) -> String {
    let chatgpt_request = ChatGPTRequest {
        model: "gpt-3.5-turbo".to_string(),
        messages: llm_messages,
    };

    let chatgpt_client = reqwest::Client::new();

    let mut chatgpt_response = None;
    loop {
        let chatgpt_future = chatgpt_client
            .post("https://api.openai.com/v1/chat/completions")
            .bearer_auth(chatgpt_api_key.clone())
            .json(&chatgpt_request)
            .send();
        if let Ok(response) = timeout(Duration::from_millis(5000), chatgpt_future).await {
            chatgpt_response = Some(response);
            break;
        }
    }
    if let Some(Ok(chatgpt_response)) = chatgpt_response {
        if let Ok(chatgpt_response) = chatgpt_response.json::<ChatGPTResponse>().await {
            return chatgpt_response.choices[0].message.clone().content;
        }
    }
    panic!("failed to make chatgpt request");
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DgLlmRequest {
    pub text: Vec<LlmMessage>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DgLlmResponse {
    pub results: String,
}

pub async fn llm_dg(llm_messages: Vec<LlmMessage>) -> String {
    let llm_request = DgLlmRequest { text: llm_messages };

    let llm_client = reqwest::Client::new();
    let llm_response: DgLlmResponse = llm_client
        .post("https://llm.sandbox.deepgram.com/deepchat")
        .json(&llm_request)
        .send()
        .await
        .expect("unable to send llm request")
        .json()
        .await
        .expect("unable to parse llm response as json");

    llm_response.results
}
