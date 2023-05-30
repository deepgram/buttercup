use crate::audio;
use crate::deepgram_response;
use crate::deepgram_response::WrappedStreamingResponse;
use crate::message::Message;
use crate::state::State;
use crate::tts;
use crate::twilio_response;
use axum::{
    extract::ws::{WebSocket, WebSocketUpgrade},
    response::IntoResponse,
    Extension,
};
use futures::channel::oneshot;
use futures::{
    sink::SinkExt,
    stream::{SplitSink, SplitStream, StreamExt},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;
use std::{convert::From, sync::Arc};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

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

// this is a quick and dumb wrapper to distinguish between chatgpt responses occuring during
// the call, and those occuring after the call
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WrappedChatGPTResponse {
    stream_sid: String,
    post_call: bool,
    chatgpt_response: ChatGPTResponse,
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

    println!("getting the pre and post call prompts, and the initial message");
    let pre_call_prompt = { state.pre_call_prompt.lock().await.clone() };
    let initial_call_message = { state.initial_call_message.lock().await.clone() };
    let post_call_prompts = { state.post_call_prompts.lock().await.clone() };

    println!("finished getting the pre and post call prompts and initial message");
    let mut chatgpt_messages = Vec::new();
    chatgpt_messages.push(ChatGPTMessage {
        role: "system".to_string(),
        content: pre_call_prompt,
    });

    // what this ugly block informs is that we should have a function for
    // streaming tts responses to twilio, since there are a number of steps involved
    // beginning of ugly block
    let initial_assistant_message = ChatGPTMessage {
        role: "assistant".to_string(),
        content: initial_call_message,
    };

    chatgpt_messages.push(initial_assistant_message.clone());

    println!("about to make initial tts request");
    let base64_encoded_mulaw =
        tts::text_to_speech(initial_assistant_message.content.clone(), state.clone()).await;
    let sending_media = twilio_response::SendingMedia::new(streamsid.clone(), base64_encoded_mulaw);
    let _ = twilio_sender
        .send(Message::Text(serde_json::to_string(&sending_media).unwrap()).into())
        .await;
    // end of ugly block
    println!("finished ugly block, including initial tts request");

    let mut running_deepgram_response = "".to_string();

    while let Some(Ok(msg)) = deepgram_receiver.next().await {
        if let tungstenite::Message::Text(msg) = msg.clone() {
            println!("Got a message from Deepgram:");
            println!("{:?}", msg);

            // if the message was an utterance end message:
            if let Ok(server_message_or_so) =
                serde_json::from_str::<deepgram_response::ServerMessageOrSo>(&msg)
            {
                if server_message_or_so.the_type == "UtteranceEnd" {
                    println!("Got an utterance end message, so will interact with ChatGPT now.");

                    if running_deepgram_response.is_empty() {
                        println!("our running deepgram transcript is empty, so will not interact with ChatGPT");
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
                        if let Ok(response) =
                            timeout(Duration::from_millis(5000), chatgpt_future).await
                        {
                            println!("got chatgpt response: {:?}", response);
                            chatgpt_response = Some(response);
                            break;
                        }
                    }
                    if let Some(Ok(chatgpt_response)) = chatgpt_response {
                        if let Ok(chatgpt_response) =
                            chatgpt_response.json::<ChatGPTResponse>().await
                        {
                            println!("got chatgpt Ok response: {:?}", chatgpt_response);
                            chatgpt_messages.push(chatgpt_response.choices[0].message.clone());

                            let base64_encoded_mulaw = tts::text_to_speech(
                                chatgpt_response.choices[0].message.content.clone(),
                                state.clone(),
                            )
                            .await;
                            let sending_media = twilio_response::SendingMedia::new(
                                streamsid.clone(),
                                base64_encoded_mulaw,
                            );

                            let _ = twilio_sender
                                .send(
                                    Message::Text(serde_json::to_string(&sending_media).unwrap())
                                        .into(),
                                )
                                .await;

                            let mut subscribers = state.subscribers.lock().await;
                            println!("sending chatgpt response to the subscribers");
                            // send the messages to all subscribers concurrently
                            let futs = subscribers.iter_mut().map(|subscriber| {
                                subscriber.send(
                                    Message::Text(
                                        serde_json::to_string(&WrappedChatGPTResponse {
                                            stream_sid: streamsid.clone(),
                                            post_call: false,
                                            chatgpt_response: chatgpt_response.clone(),
                                        })
                                        .unwrap(),
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

            // otherwise, if our message was an ASR message:
            let deepgram_response: Result<deepgram_response::StreamingResponse, _> =
                serde_json::from_str(&msg);
            if let Ok(deepgram_response) = deepgram_response {
                if deepgram_response.channel.alternatives.is_empty() {
                    println!("empty alternatives");
                    continue;
                }

                if !deepgram_response.is_final {
                    println!("not a final response");
                    continue;
                }

                {
                    println!("sending deepgram response to all subscribers");
                    let mut subscribers = state.subscribers.lock().await;
                    let futs = subscribers.iter_mut().map(|subscriber| {
                        subscriber.send(
                            Message::Text(
                                serde_json::to_string(&WrappedStreamingResponse {
                                    stream_sid: streamsid.clone(),
                                    streaming_response: deepgram_response.clone(),
                                })
                                .expect("wrapped streaming responses should be serializable"),
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
                        .filter_map(|(subscriber, result)| result.is_ok().then_some(subscriber))
                        .collect();
                }

                let transcript = deepgram_response.channel.alternatives[0].transcript.clone();

                if !transcript.is_empty() {
                    println!("our transcript isn't empty, so appending it to our running deepgram transcript");
                    running_deepgram_response = format!("{running_deepgram_response} {transcript}");
                    let _ = twilio_sender
                        .send(
                            Message::Text(
                                serde_json::to_string(
                                    &json!({ "event": "clear", "streamSid": streamsid }),
                                )
                                .unwrap(),
                            )
                            .into(),
                        )
                        .await;
                }
            }
        }
    }

    // send post call chatgpt responses to post call chatgpt prompts
    for post_call_prompt in post_call_prompts {
        chatgpt_messages.push(ChatGPTMessage {
            role: "user".to_string(),
            content: post_call_prompt.clone(),
        });

        let chatgpt_request = ChatGPTRequest {
            model: "gpt-3.5-turbo".to_string(),
            messages: chatgpt_messages.clone(),
        };

        println!(
            "about to make post call chatgpt request: {:?}",
            chatgpt_request
        );
        let chatgpt_client = reqwest::Client::new();
        let mut chatgpt_response = None;
        loop {
            println!("trying to make the chatgpt request");
            let chatgpt_future = chatgpt_client
                .post("https://api.openai.com/v1/chat/completions")
                .bearer_auth(chatgpt_api_key.clone())
                .json(&chatgpt_request)
                .send();
            if let Ok(response) = timeout(Duration::from_millis(5000), chatgpt_future).await {
                println!("got chatgpt response: {:?}", response);
                chatgpt_response = Some(response);
                break;
            }
        }
        if let Some(Ok(chatgpt_response)) = chatgpt_response {
            if let Ok(chatgpt_response) = chatgpt_response.json::<ChatGPTResponse>().await {
                println!("got chatgpt Ok response: {:?}", chatgpt_response);
                chatgpt_messages.push(chatgpt_response.choices[0].message.clone());
                let mut subscribers = state.subscribers.lock().await;
                println!("sending chatgpt response to the subscribers");
                // send the messages to all subscribers concurrently
                let futs = subscribers.iter_mut().map(|subscriber| {
                    subscriber.send(
                        Message::Text(
                            serde_json::to_string(&WrappedChatGPTResponse {
                                stream_sid: streamsid.clone(),
                                post_call: true,
                                chatgpt_response: chatgpt_response.clone(),
                            })
                            .unwrap(),
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
                    .filter_map(|(subscriber, result)| result.is_ok().then_some(subscriber))
                    .collect();
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
