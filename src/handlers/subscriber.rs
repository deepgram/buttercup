use crate::message::Message;
use crate::state::State;
use axum::{
    extract::ws::{WebSocket, WebSocketUpgrade},
    response::IntoResponse,
    Extension,
};
use std::sync::Arc;

pub async fn subscriber_handler(
    ws: WebSocketUpgrade,
    Extension(state): Extension<Arc<State>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: Arc<State>) {
    println!("subscriber.rs handle_socket");
    let mut subscribers = state.subscribers.lock().await;
    // send these keys (which will be twilio streamsids) to the client
    let keys = subscribers.keys().map(|key| key.to_string()).collect();
    socket
        .send(Message::Text(keys).into())
        .await
        .expect("Failed to send streamsids to client.");

    // wait for the first message from the client
    // and interpret it as the streamsid to subscribe to
    if let Some(Ok(msg)) = socket.recv().await {
        let msg = Message::from(msg);
        if let Message::Text(streamsid) = msg {
            let streamsid = streamsid.trim();
            if let Some(subscribers) = subscribers.get_mut(streamsid) {
                println!("pushing subscriber socket");
                subscribers.push(socket);
            }
        }
    }
}
