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

async fn handle_socket(socket: WebSocket, state: Arc<State>) {
    println!("subscriber.rs handle_socket");
    {
        let mut subscribers = state.subscribers.lock().await;
        println!("got the lock on subscribers");
        subscribers.push(socket);
    }
}
