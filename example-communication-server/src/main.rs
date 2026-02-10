mod client;
mod websocket;
mod handlers;
mod webserver;

use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
use crate::{client::{ClientMap}, webserver::webserver_loop};
//use example-communication-common::generate_challenge;


#[tokio::main]
async fn main() {
    let clients: ClientMap = Arc::new(Mutex::new(HashMap::new()));
    webserver_loop(clients).await;
}
