use axum::routing::{get, post};
use axum::{Extension, Router};
use axum_server::tls_rustls::RustlsConfig;
use futures::lock::Mutex;
use std::sync::Arc;
use tower_http::cors::CorsLayer;

mod audio;
mod deepgram_response;
mod handlers;
mod llm;
mod message;
mod state;
mod tts;
mod twilio_response;

#[tokio::main]
async fn main() {
    let proxy_url = std::env::var("PROXY_URL").unwrap_or_else(|_| "127.0.0.1:5000".to_string());

    let deepgram_url = std::env::var("DEEPGRAM_URL").unwrap_or_else(|_| {
        "wss://api.deepgram.com/v1/listen?encoding=mulaw&sample_rate=8000&punctuate=true&model=nova-smart-format&utterance_end_ms=1000&interim_results=true".to_string()
    });

    let deepgram_api_key =
        std::env::var("DEEPGRAM_API_KEY").expect("Using this server requires a Deepgram API Key.");

    let chatgpt_api_key =
        std::env::var("CHATGPT_API_KEY").expect("Using this server requires a ChatGPT API Key.");

    let cert_pem = std::env::var("CERT_PEM").ok();
    let key_pem = std::env::var("KEY_PEM").ok();

    let config = match (cert_pem, key_pem) {
        (Some(cert_pem), Some(key_pem)) => Some(
            RustlsConfig::from_pem_file(cert_pem, key_pem)
                .await
                .expect("Failed to make RustlsConfig from cert/key pem files."),
        ),
        (None, None) => None,
        _ => {
            panic!("Failed to start - invalid cert/key.")
        }
    };

    let pre_call_prompt =
        Mutex::new("You are an insurance agent for home and auto insurance.".to_string());
    let initial_call_message = Mutex::new("Hello, how may I help you today?".to_string());
    let introspection_prompt = Mutex::new("I am an insurance agent and you are my coach, read this conversation between a customer and myself and tell me if there is a problem with the way I am communicating - if I am communicating well just say \"hamburger pineapple\"".to_string());
    let introspection_carry_on = Mutex::new(Some("hamburger pineapple".to_string()));
    let post_call_prompts = Mutex::new(vec![
        "What did the customer call about?".to_string(),
        "Did the insurance agent do a good job?".to_string(),
    ]);

    let state = Arc::new(state::State {
        deepgram_url,
        deepgram_api_key,
        llm: state::Llm::ChatGPT(chatgpt_api_key),
        tts: state::Tts::Aws,
        subscribers: Mutex::new(Vec::new()),
        pre_call_prompt,
        initial_call_message,
        introspection_prompt,
        introspection_carry_on,
        post_call_prompts,
    });

    let app = Router::new()
        .route("/twilio", get(handlers::twilio::twilio_handler))
        .route("/client", get(handlers::subscriber::subscriber_handler))
        .route(
            "/pre-call-prompt",
            get(handlers::prompts::get_pre_call_prompt),
        )
        .route(
            "/initial-call-message",
            get(handlers::prompts::get_initial_call_message),
        )
        .route(
            "/introspection-prompt",
            get(handlers::prompts::get_introspection_prompt),
        )
        .route(
            "/post-call-prompts",
            get(handlers::prompts::get_post_call_prompts),
        )
        .route(
            "/pre-call-prompt",
            post(handlers::prompts::post_pre_call_prompt),
        )
        .route(
            "/initial-call-message",
            post(handlers::prompts::post_initial_call_message),
        )
        .route(
            "/introspection-prompt",
            post(handlers::prompts::post_introspection_prompt),
        )
        .route(
            "/post-call-prompts",
            post(handlers::prompts::post_post_call_prompts),
        )
        .layer(CorsLayer::permissive())
        .layer(Extension(state));

    match config {
        Some(config) => {
            axum_server::bind_rustls(proxy_url.parse().unwrap(), config)
                .serve(app.into_make_service())
                .await
                .unwrap();
        }
        None => {
            axum_server::bind(proxy_url.parse().unwrap())
                .serve(app.into_make_service())
                .await
                .unwrap();
        }
    }
}
