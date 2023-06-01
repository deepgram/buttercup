use crate::state::State;
use axum::extract;
use axum::http::StatusCode;
use axum::Extension;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Serialize, Deserialize)]
pub struct Prompt {
    prompt: String,
}

#[derive(Serialize, Deserialize)]
pub struct InitialCallMessage {
    message: String,
}

#[derive(Serialize, Deserialize)]
pub struct Prompts {
    prompts: Vec<String>,
}

pub async fn get_pre_call_prompt(Extension(state): Extension<Arc<State>>) -> Json<Prompt> {
    let pre_call_prompt = state.pre_call_prompt.lock().await;
    Json(Prompt {
        prompt: pre_call_prompt.clone(),
    })
}

pub async fn get_initial_call_message(
    Extension(state): Extension<Arc<State>>,
) -> Json<InitialCallMessage> {
    let initial_call_message = state.initial_call_message.lock().await;
    Json(InitialCallMessage {
        message: initial_call_message.clone(),
    })
}

pub async fn get_introspection_prompt(Extension(state): Extension<Arc<State>>) -> Json<Prompt> {
    let introspection_prompt = state.introspection_prompt.lock().await;
    Json(Prompt {
        prompt: introspection_prompt.clone(),
    })
}

pub async fn get_post_call_prompts(Extension(state): Extension<Arc<State>>) -> Json<Prompts> {
    let post_call_prompts = state.post_call_prompts.lock().await;
    Json(Prompts {
        prompts: post_call_prompts.clone(),
    })
}

pub async fn post_pre_call_prompt(
    extract::Json(payload): extract::Json<Prompt>,
    Extension(state): Extension<Arc<State>>,
) -> StatusCode {
    let mut pre_call_prompt = state.pre_call_prompt.lock().await;
    *pre_call_prompt = payload.prompt;

    StatusCode::OK
}

pub async fn post_initial_call_message(
    extract::Json(payload): extract::Json<InitialCallMessage>,
    Extension(state): Extension<Arc<State>>,
) -> StatusCode {
    let mut initial_call_message = state.initial_call_message.lock().await;
    *initial_call_message = payload.message;

    StatusCode::OK
}

pub async fn post_introspection_prompt(
    extract::Json(payload): extract::Json<Prompt>,
    Extension(state): Extension<Arc<State>>,
) -> StatusCode {
    let mut introspection_prompt = state.introspection_prompt.lock().await;
    *introspection_prompt = payload.prompt;

    StatusCode::OK
}

pub async fn post_post_call_prompts(
    extract::Json(payload): extract::Json<Prompts>,
    Extension(state): Extension<Arc<State>>,
) -> StatusCode {
    let mut post_call_prompts = state.post_call_prompts.lock().await;
    *post_call_prompts = payload.prompts;

    StatusCode::OK
}
