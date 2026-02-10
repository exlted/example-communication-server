use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use warp::Rejection;
use warp::ws::Message;
use example_communication_common::ConnectionInfo;

pub type Result<T> = std::result::Result<T, Rejection>;
pub type ClientSendChannel = mpsc::UnboundedSender<std::result::Result<Message, warp::Error>>;

#[derive(Debug, Clone)]
pub struct Client {
    pub client_id: Option<ConnectionInfo>,
    pub sender: ClientSendChannel,
}
pub type ClientMap = Arc<Mutex<HashMap<String, Client>>>;