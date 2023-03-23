use crate::audio;
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
use serde::{Deserialize, Serialize};
use std::time::Duration;
use std::{convert::From, sync::Arc};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use serde_json::json;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatGPTRequest {
    model: String,
    messages: Vec<ChatGPTMessage>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatGPTMessage {
    role: String,
    content: String,
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
    message: ChatGPTMessage,
    finish_reason: String,
}

pub async fn twilio_handler(
    ws: WebSocketUpgrade,
    Extension(state): Extension<Arc<State>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<State>) {
    println!("twilio.rs handle_socket");
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
        state.chatgpt_api_key.clone(),
        Arc::clone(&state),
    ));
    tokio::spawn(handle_from_twilio_ws(
        this_receiver,
        deepgram_sender,
        streamsid_tx,
        Arc::clone(&state),
    ));
}

async fn handle_from_deepgram_ws(
    mut deepgram_receiver: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    mut twilio_sender: SplitSink<WebSocket, axum::extract::ws::Message>,
    streamsid_rx: oneshot::Receiver<String>,
    chatgpt_api_key: String,
    state: Arc<State>,
) {
    println!("handle_from_deepgram_ws");
    let streamsid = streamsid_rx
        .await
        .expect("Failed to receive streamsid from handle_from_twilio_ws.");

    let mut chatgpt_messages = Vec::new();
    chatgpt_messages.push(ChatGPTMessage {
        role: "system".to_string(),
        content:
            "You work at a pizza shop answering phone calls from customers trying to order pizza."
                .to_string(),
    });

    let mut running_deepgram_response = "".to_string();

    while let Some(Ok(msg)) = deepgram_receiver.next().await {
        if let tungstenite::Message::Text(msg) = msg.clone() {
            let deepgram_response: Result<deepgram_response::StreamingResponse, _> =
                serde_json::from_str(&msg);
            if let Ok(deepgram_response) = deepgram_response {
                if deepgram_response.channel.alternatives.is_empty() {
                    continue;
                }

                {
                    let mut subscribers = state.subscribers.lock().await;
                    if let Some(subscribers) = subscribers.get_mut(&streamsid) {
                        println!("sending deepgram response to the subscribers");
                        // send the messages to all subscribers concurrently
                        let futs = subscribers.iter_mut().map(|subscriber| {
                            subscriber.send(Message::Text(msg.clone()).into())
                        });
                        let results = futures::future::join_all(futs).await;

                        // if we successfully sent a message then the subscriber is still connected
                        // other subscribers should be removed
                        *subscribers = subscribers
                            .drain(..)
                            .zip(results)
                            .filter_map(|(subscriber, result)| {
                                result.is_ok().then_some(subscriber)
                            })
                            .collect();
                    }
                }

                let transcript = deepgram_response.channel.alternatives[0].transcript.clone();

                if !transcript.is_empty() {
                    running_deepgram_response = format!("{running_deepgram_response} {transcript}");
                    let _ = twilio_sender
                        .send(
                            Message::Text(
                                serde_json::to_string(&json!({ "event": "clear", "streamSid": streamsid })).unwrap(),
                            )
                            .into(),
                        )
                        .await;
                }

                if !deepgram_response.speech_final.unwrap_or(false) {
                    continue;
                }

                if running_deepgram_response.is_empty() {
                    continue;
                }

                chatgpt_messages.push(ChatGPTMessage {
                    role: "user".to_string(),
                    content: running_deepgram_response.clone(),
                });

                running_deepgram_response = "".to_string();

                let chatgpt_request = ChatGPTRequest {
                    model: "gpt-3.5-turbo".to_string(),
                    messages: chatgpt_messages.clone(),
                };

                println!("about to make chatgpt request: {:?}", chatgpt_request);
                let chatgpt_client = reqwest::Client::new();
                let mut chatgpt_response = None;
                loop {
                    println!("trying to make the chatgpt request");
                    let chatgpt_future = chatgpt_client
                        .post("https://api.openai.com/v1/chat/completions")
                        .bearer_auth(chatgpt_api_key.clone())
                        .json(&chatgpt_request)
                        .send();
                    if let Ok(response) = timeout(Duration::from_millis(5000), chatgpt_future).await
                    {
                        chatgpt_response = Some(response);
                        break;
                    }
                }
                if let Some(Ok(chatgpt_response)) = chatgpt_response {
                    if let Ok(chatgpt_response) = chatgpt_response.json::<ChatGPTResponse>().await {
                        println!("got chatgpt response: {:?}", chatgpt_response);
                        chatgpt_messages.push(chatgpt_response.choices[0].message.clone());

                        let shared_config = aws_config::from_env().load().await;
                        let client = Client::new(&shared_config);

                        if let Ok(polly_response) = client
                            .synthesize_speech()
                            .output_format(OutputFormat::Pcm)
                            .sample_rate("8000")
                            .text(chatgpt_response.choices[0].message.content.clone())
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

                                let mut subscribers = state.subscribers.lock().await;
                                if let Some(subscribers) = subscribers.get_mut(&streamsid) {
                                    println!("sending chatgpt response to the subscribers");
                                    // send the messages to all subscribers concurrently
                                    let futs = subscribers.iter_mut().map(|subscriber| {
                                        subscriber.send(
                                            Message::Text(
                                                serde_json::to_string(&chatgpt_response).unwrap(),
                                            )
                                            .into(),
                                        )
                                    });
                                    let results = futures::future::join_all(futs).await;

                                    // if we successfully sent a message then the subscriber is still connected
                                    // other subscribers should be removed
                                    *subscribers = subscribers
                                        .drain(..)
                                        .zip(results)
                                        .filter_map(|(subscriber, result)| {
                                            result.is_ok().then_some(subscriber)
                                        })
                                        .collect();
                                }
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
    state: Arc<State>,
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

                        // make a new set of subscribers for this call, using the stream sid as the key
                        state
                            .subscribers
                            .lock()
                            .await
                            .entry(start.stream_sid)
                            .or_default();
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
