use axum::{routing::get, Extension, Router};
use axum_server::tls_rustls::RustlsConfig;
use futures::lock::Mutex;
use std::sync::Arc;

mod audio;
mod deepgram_response;
mod handlers;
mod message;
mod state;
mod twilio_response;

#[tokio::main]
async fn main() {
    let proxy_url = std::env::var("PROXY_URL").unwrap_or_else(|_| "127.0.0.1:5000".to_string());

    let deepgram_url = std::env::var("DEEPGRAM_URL").unwrap_or_else(|_| {
        "wss://api.deepgram.com/v1/listen?encoding=mulaw&sample_rate=8000&punctuate=true&tier=enhanced&endpointing=1500".to_string()
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

    let pre_call_prompt = Mutex::new("You work at a pizza shop called Paparazzi's answering phone calls from customers trying to order pizza. You only offer delivery, and you need to get the customer's address.".to_string());
    let initial_call_message =
        Mutex::new("Hello, this is Paparazzi's, what would you like to order?".to_string());
    let post_call_prompts = Mutex::new(vec![
        "What did the customer order?".to_string(),
        "What is the customer's address?".to_string(),
    ]);

    let state = Arc::new(state::State {
        deepgram_url,
        deepgram_api_key,
        chatgpt_api_key,
        subscribers: Mutex::new(Vec::new()),
        pre_call_prompt,
        initial_call_message,
        post_call_prompts,
    });

    let app = Router::new()
        .route("/twilio", get(handlers::twilio::twilio_handler))
        .route("/client", get(handlers::subscriber::subscriber_handler))
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
