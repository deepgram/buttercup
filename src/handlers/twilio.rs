use crate::audio;
use crate::cleverbot_response;
use crate::deepgram_response;
use crate::message::Message;
use crate::state::State;
use crate::twilio_response;
use aws_sdk_polly::model::{OutputFormat, VoiceId};
use aws_sdk_polly::Client;
use axum::{
    extract::ws::{WebSocket, WebSocketUpgrade},
    response::IntoResponse,
    Extension,
};
use base64::{engine::general_purpose, Engine};
use futures::channel::oneshot;
use futures::{
    sink::SinkExt,
    stream::{SplitSink, SplitStream, StreamExt},
};
use std::{convert::From, sync::Arc};
use tokio::net::TcpStream;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

pub async fn twilio_handler(
    ws: WebSocketUpgrade,
    Extension(state): Extension<Arc<State>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<State>) {
    let (this_sender, this_receiver) = socket.split();

    // prepare the deepgram connection request with the api key authentication
    let builder = http::Request::builder()
        .method(http::Method::GET)
        .uri(&state.deepgram_url);
    let builder = builder.header("Authorization", format!("Token {}", state.deepgram_api_key));
    let request = builder
        .body(())
        .expect("Failed to build a connection request to Deepgram.");

    // connect to deepgram
    let (deepgram_socket, _) = connect_async(request)
        .await
        .expect("Failed to connect to Deepgram.");
    let (deepgram_sender, deepgram_reader) = deepgram_socket.split();

    let (streamsid_tx, streamsid_rx) = oneshot::channel::<String>();

    tokio::spawn(handle_from_deepgram_ws(
        deepgram_reader,
        this_sender,
        streamsid_rx,
        state.cleverbot_api_key.clone(),
    ));
    tokio::spawn(handle_from_twilio_ws(
        this_receiver,
        deepgram_sender,
        streamsid_tx,
    ));
}

async fn handle_from_deepgram_ws(
    mut deepgram_receiver: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    mut twilio_sender: SplitSink<WebSocket, axum::extract::ws::Message>,
    streamsid_rx: oneshot::Receiver<String>,
    cleverbot_api_key: String,
) {
    let streamsid = streamsid_rx
        .await
        .expect("Failed to receive streamsid from handle_from_twilio_ws.");

    let mut cleverbot_cs = None;

    while let Some(Ok(msg)) = deepgram_receiver.next().await {
        if let tungstenite::Message::Text(msg) = msg.clone() {
            let deepgram_response: Result<deepgram_response::StreamingResponse, _> =
                serde_json::from_str(&msg);
            if let Ok(deepgram_response) = deepgram_response {
                if deepgram_response.channel.alternatives.is_empty() {
                    continue;
                }

                let transcript = deepgram_response.channel.alternatives[0].transcript.clone();

                let mut cleverbot_url = format!(
                    "https://www.cleverbot.com/getreply?key={}",
                    cleverbot_api_key
                );

                if let Some(cs) = cleverbot_cs.clone() {
                    cleverbot_url = format!("{}&cs={}", cleverbot_url, cs);
                }
                cleverbot_url = format!("{}&input={}", cleverbot_url, transcript);

                if let Ok(cleverbot_response) = reqwest::get(cleverbot_url).await {
                    if let Ok(cleverbot_response) = cleverbot_response
                        .json::<cleverbot_response::CleverbotResponse>()
                        .await
                    {
                        cleverbot_cs = Some(cleverbot_response.cs);

                        let shared_config = aws_config::from_env().load().await;
                        let client = Client::new(&shared_config);

                        if let Ok(polly_response) = client
                            .synthesize_speech()
                            .output_format(OutputFormat::Pcm)
                            .sample_rate("8000")
                            .text(cleverbot_response.output)
                            .voice_id(VoiceId::Joanna)
                            .send()
                            .await
                        {
                            if let Ok(pcm) = polly_response
                                .audio_stream
                                .collect()
                                .await
                                .map(|aggregated_bytes| aggregated_bytes.to_vec())
                            {
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

                                // base64 encode the mulaw, wrap it in a Twilio media message, and send it to Twilio
                                let base64_encoded_mulaw =
                                    general_purpose::STANDARD.encode(&mulaw_samples);

                                let sending_media = twilio_response::SendingMedia::new(
                                    streamsid.clone(),
                                    base64_encoded_mulaw,
                                );

                                let _ = twilio_sender
                                    .send(
                                        Message::Text(
                                            serde_json::to_string(&sending_media).unwrap(),
                                        )
                                        .into(),
                                    )
                                    .await;
                            }
                        }
                    }
                }
            }
        }
    }
}

async fn handle_from_twilio_ws(
    mut this_receiver: SplitStream<WebSocket>,
    mut deepgram_sender: SplitSink<
        WebSocketStream<MaybeTlsStream<TcpStream>>,
        tokio_tungstenite::tungstenite::Message,
    >,
    streamsid_tx: oneshot::Sender<String>,
) {
    let mut buffer_data = audio::BufferData {
        inbound_buffer: Vec::new(),
        inbound_last_timestamp: 0,
    };

    // wrap our oneshot in an Option because we will need it in a loop
    let mut streamsid_tx = Some(streamsid_tx);

    while let Some(Ok(msg)) = this_receiver.next().await {
        let msg = Message::from(msg);
        if let Message::Text(msg) = msg {
            let event: Result<twilio_response::Event, _> = serde_json::from_str(&msg);
            if let Ok(event) = event {
                match event.event_type {
                    twilio_response::EventType::Start(start) => {
                        // sending this streamsid on our oneshot will let `handle_from_deepgram_ws` know the streamsid
                        if let Some(streamsid_tx) = streamsid_tx.take() {
                            streamsid_tx
                                .send(start.stream_sid.clone())
                                .expect("Failed to send streamsid to handle_to_game_rx.");
                        }
                    }
                    twilio_response::EventType::Media(media) => {
                        if let Some(audio) = audio::process_twilio_media(media, &mut buffer_data) {
                            // send the audio on to deepgram
                            if deepgram_sender
                                .send(Message::Binary(audio).into())
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
}
