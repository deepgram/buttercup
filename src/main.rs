use axum::{routing::get, Extension, Router};
use axum_server::tls_rustls::RustlsConfig;
use std::sync::Arc;

mod audio;
mod cleverbot_response;
mod deepgram_response;
mod handlers;
mod message;
mod state;
mod twilio_response;

#[tokio::main]
async fn main() {
    let proxy_url = std::env::var("PROXY_URL").unwrap_or_else(|_| "127.0.0.1:5000".to_string());

    let deepgram_url = std::env::var("DEEPGRAM_URL").unwrap_or_else(|_| {
        "wss://api.deepgram.com/v1/listen?encoding=mulaw&sample_rate=8000&numerals=true".to_string()
    });

    let deepgram_api_key =
        std::env::var("DEEPGRAM_API_KEY").expect("Using this server requires a Deepgram API Key.");

    let cleverbot_api_key = std::env::var("CLEVERBOT_API_KEY")
        .expect("Using this server requires a Cleverbot API Key.");

    let twilio_phone_number = std::env::var("TWILIO_PHONE_NUMBER")
        .expect("Using this server requires a Twilio phone number.");

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

    let state = Arc::new(state::State {
        deepgram_url,
        deepgram_api_key,
        cleverbot_api_key,
        twilio_phone_number,
    });

    let app = Router::new()
        .route("/twilio", get(handlers::twilio::twilio_handler))
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
