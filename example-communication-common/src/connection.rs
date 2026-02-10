use std::ops::Deref;
use std::sync::Arc;
use futures_util::{SinkExt, StreamExt};
use tokio::{select};
use tokio_tungstenite::{WebSocketStream, MaybeTlsStream, connect_async};
use tokio_tungstenite::tungstenite::{handshake::client::Response, error::Error};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
pub use tokio::{
    sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    task::spawn
};
use tokio::sync::mpsc::error::SendError;
use tokio::sync::Notify;
use tokio_stream::wrappers::UnboundedReceiverStream;
use crate::{CommandType, Destination, ThreadSafe, WebSocketMessage};
pub use tokio_tungstenite::tungstenite::Message;

type Websocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub trait ConnectionSettings {
    fn get_url(&self) -> String;
    fn get_key(&self) -> String;
}

pub trait Status {
    fn update_status(&self, message: String);
}

pub trait Sender {
    fn try_send(&self, message: WebSocketMessage) -> Result<(), SendError<WebSocketMessage>>;
    fn drop_connection(&mut self);
    fn set_connection(&mut self, new_sender: UnboundedSender<WebSocketMessage>);
}

async fn connect(connection_info: &impl ConnectionSettings) -> Result<(WebSocketStream<MaybeTlsStream<TcpStream>>, Response), Error>{
    let mut request = connection_info.get_url().into_client_request()?;
    request.headers_mut().insert("Authorization", connection_info.get_key().parse()?);
    
    Ok(connect_async(request).await?)
}

async fn handle_packet(packet: WebSocketMessage, from_server_sender: UnboundedSender<WebSocketMessage>) -> Option<WebSocketMessage> {
    match packet.command {
        // From Server
        //CommandType::Welcome { .. } => {}
        //CommandType::ActiveConnections { .. } => {}
        //CommandType::RequestConnectionInfo => {}
        // Unexpected Packet Types
        CommandType::GetConnections { .. } => {None}
        CommandType::SetConnectionInfo { .. } => {None}
        // Pass Through to Client
        _ => {
            if from_server_sender.is_closed() {
                return None;
            } else {
                from_server_sender.send(packet).unwrap();
            }
            None
        }
    }
}

async fn websocket_loop(socket: Websocket, from_server_sender: UnboundedSender<WebSocketMessage>, to_server_receiver: UnboundedReceiver<WebSocketMessage>) {

    let mut to_server_stream = UnboundedReceiverStream::new(to_server_receiver);
    let mut connected = true;
    let (mut socket_tx, mut socket_rx) = socket.split();


    while connected {
        select! {
            message = to_server_stream.next() => {
                // Client wants to send something out
                if let Some(message) = message {
                    if matches!(message.command, CommandType::Disconnect)
                    {
                        connected = false;
                        continue;
                    }

                    let reply = serde_json::to_string(&message).expect("Failed to serialize Outgoing Message");
                    socket_tx.send(Message::text(reply)).await.expect("Failed to send message");
                }
            }
            message = socket_rx.next() => {
                if from_server_sender.is_closed()
                {
                    connected = false;
                    continue;
                }

                // Something Arrived from the Server
                if let Some(message) = message {
                    if !message.is_ok() {
                        continue
                    }
                    let message = message.unwrap();
                    if !message.is_text() {
                        continue
                    }
                    let packet: serde_json::error::Result<WebSocketMessage> = serde_json::from_str(message.to_text().unwrap());
                    if let Ok(packet) = packet {
                        let reply = handle_packet(packet, from_server_sender.clone()).await;
                        if let Some(reply) = reply {
                            let reply = serde_json::to_string(&reply).expect("Failed to serialize Outgoing Message");
                            socket_tx.send(Message::text(reply)).await.expect("Failed to send message");
                        }
                    }
                }
            }
        }
    }

    socket_tx.close().await.expect("Failed to close socket");
}

async fn start_websocket_connection(connection_info: &impl ConnectionSettings) -> Result<(UnboundedSender<WebSocketMessage>, UnboundedReceiver<WebSocketMessage>), Error> {
    let test = connect(connection_info).await;
    if test.is_err()
    {
        return Err(test.unwrap_err())
    }

    let (connection, _) = test?;
    let (from_server_sender, from_server_receiver) = unbounded_channel::<WebSocketMessage>();
    let (to_server_sender, to_server_receiver) = unbounded_channel::<WebSocketMessage>();

    spawn(websocket_loop(connection, from_server_sender, to_server_receiver));

    Ok((to_server_sender, from_server_receiver))
}


pub async fn connect_to_server_loop(connection_info: ThreadSafe<impl ConnectionSettings>, connection_state_changed: Arc<Notify>, status: &impl Status) -> (UnboundedSender<WebSocketMessage>, UnboundedReceiver<WebSocketMessage>) {
    let mut connection: Option<(UnboundedSender<WebSocketMessage>, UnboundedReceiver<WebSocketMessage>)> = None;

    while connection.is_none() {
        let connection_state = {
            start_websocket_connection(connection_info.lock().await.deref()).await
        };
        if connection_state.is_err() {
            status.update_status(connection_state.err().unwrap().to_string());
            connection_state_changed.notified().await;
            continue;
        }
        status.update_status("Connected".to_string());
        connection = Some(connection_state.unwrap());
    }

    connection.unwrap()
}


pub async fn reconnect(sender: ThreadSafe<impl Sender>, connection_info: ThreadSafe<impl ConnectionSettings>, mut from_server: UnboundedReceiver<WebSocketMessage>, connection_state_changed: Arc<Notify>, status: &impl Status) -> UnboundedReceiver<WebSocketMessage> {
    // Close connection and re-connect
    sender.lock().await.try_send(WebSocketMessage{
        command: CommandType::Disconnect,
        destination: Destination::None
    }).expect("Failed to send message");

    sender.lock().await.drop_connection();
    from_server.close();

    let (to_server, new_from_server) = connect_to_server_loop(connection_info, connection_state_changed.clone(), status).await;

    sender.lock().await.set_connection(to_server);

    new_from_server
}