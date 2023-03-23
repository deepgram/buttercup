use crate::message::Message;
use crate::state::State;
use axum::{
    extract::ws::{WebSocket, WebSocketUpgrade},
    response::IntoResponse,
    Extension,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Serialize, Deserialize, Debug)]
pub struct Keys {
    keys: Vec<String>,
}

pub async fn subscriber_handler(
    ws: WebSocketUpgrade,
    Extension(state): Extension<Arc<State>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: Arc<State>) {
    println!("subscriber.rs handle_socket");
    {
        let mut subscribers = state.subscribers.lock().await;
        println!("got the lock on subscribers");
        // send these keys (which will be twilio streamsids) to the client
        let keys: Vec<String> = subscribers.keys().map(|key| key.to_string()).collect();
        let keys = Keys { keys };
        println!("got the subscribers keys: {:?}", keys);
        socket
            .send(Message::Text(serde_json::to_string(&keys).unwrap()).into())
            .await
            .expect("Failed to send streamsids to client.");
        println!("just sent subscribers keys");
    }

    // wait for the first message from the client
    // and interpret it as the streamsid to subscribe to
    if let Some(Ok(msg)) = socket.recv().await {
        let msg = Message::from(msg);
        if let Message::Text(streamsid) = msg {
            let streamsid = streamsid.trim();
            let mut subscribers = state.subscribers.lock().await;

            if let Some(subscribers) = subscribers.get_mut(streamsid) {
                println!("pushing subscriber socket");
                subscribers.push(socket);
            }
        }
    }
}
