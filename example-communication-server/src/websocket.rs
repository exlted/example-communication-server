use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use uuid::Uuid;
use warp::ws::{Message, WebSocket};
use crate::client::{Client, ClientMap, ClientSendChannel};
use futures::{StreamExt, FutureExt};
use tokio::sync::mpsc::UnboundedSender;
use warp::Error;
use example_communication_common::{CommandType, ConnectionInfo, Destination, WebSocketMessage};

async fn send_packet(channel: &UnboundedSender<Result<Message, Error>>, packet: WebSocketMessage) {
    let reply = serde_json::to_string(&packet).expect("Failed to serialize GetConnections Reply");
    channel.send(Ok(Message::text(reply))).ok();
}

pub async fn client_connection(ws: WebSocket, clients: ClientMap) {
    println!("establishing example-communication-client connection... {:?}", ws);
    let (client_ws_sender, mut client_ws_rcv) = ws.split();
    let (client_sender, client_rcv) = mpsc::unbounded_channel();
    let client_rcv = UnboundedReceiverStream::new(client_rcv);
    tokio::task::spawn(client_rcv.forward(client_ws_sender).map(|result| {
        if let Err(e) = result {
            println!("error sending websocket msg: {}", e);
        }
    }));
    let uuid = Uuid::new_v4().to_string();

    // Send a Welcome packet including the UUID
    let welcome_packet = WebSocketMessage {
        command: CommandType::Welcome { uuid: uuid.clone() },
        destination: Destination::Single{destination_uuid: uuid.clone()},
    };

    send_packet(&client_sender, welcome_packet).await;

    let new_client = Client {
        client_id: None,
        sender: client_sender,
    };

    {
        clients.lock().await.insert(uuid.clone(), new_client);
    }

    while let Some(result) = client_ws_rcv.next().await {
        let msg = match result {
            Ok(msg) => msg,
            Err(e) => {
                println!("error receiving message for id {}): {}", uuid.clone(), e);
                break;
            }
        };

        if msg.is_close()
        {
            break;
        }
        if msg.is_text()
        {
            client_msg(&uuid, msg, &clients).await;
        }

    }

    {
        let mut locked_clients = clients.lock().await;
        locked_clients.remove(&uuid);
        for client in locked_clients.values_mut() {
            send_packet(&client.sender, WebSocketMessage{
                command: CommandType::NotifyDisconnect {
                    uuid: uuid.clone(),
                },
                destination: Destination::All,
            }).await;
        }
    }

    println!("{} disconnected", uuid);
}
async fn client_msg(client_id: &str, msg: Message, clients: &ClientMap) {
    println!("received message from {}: {:?}", client_id, msg);
    let message = match msg.to_str() {
        Ok(v) => v,
        Err(_) => return,
    };

    let deserialized = serde_json::from_str::<WebSocketMessage>(message);
    if deserialized.is_err() {
        return
    }

    let data = deserialized.unwrap();
    match data.command {
        // Server -> Client Messages
        CommandType::Welcome { .. } => {
            // Unexpected Server should send to Client
        }
        CommandType::ActiveConnections { .. } => {
            // Unexpected Server should send to Client
        }
        // Client -> Server Messages
        CommandType::GetConnections { reply_uuid } => {
            let locked = clients.lock().await;
            let mut connections: Vec<ConnectionInfo> = vec!();
            let mut sender: Option<ClientSendChannel> = None;
            for (uuid, connection_info) in &*locked {
                if let Some(client_id) = &connection_info.client_id {
                    if reply_uuid == *uuid
                    {
                        sender = Some(connection_info.sender.clone());
                    }

                    let connection = client_id.clone();
                    connections.push(connection);
                }
            }

            if let Some(sender) = sender {
                send_packet(&sender, WebSocketMessage {
                    destination: data.destination,
                    command: CommandType::ActiveConnections {
                        users: connections,
                    }
                }).await;
            }
        }
        CommandType::SetConnectionInfo { info } => {
            // Update the Connection's Client Info with the new one it just sent in
            let mut locked = clients.lock().await;

            let uuid = info.uuid.clone();
            let connection = locked.get(&uuid);
            if let Some(connection) = connection.clone() {
                let new_client = Client {
                    client_id: Some(info.clone()),
                    sender: connection.sender.clone(),
                };

                send_packet(&new_client.sender, WebSocketMessage{
                    destination: Destination::Single { destination_uuid: uuid.clone() },
                    command: CommandType::Ack
                }).await;

                locked.remove(&uuid);
                locked.insert(uuid.clone(), new_client.clone());

                for client in locked.values() {
                    if let Some(client_info) = &client.client_id {
                        if client_info.uuid != uuid {
                            send_packet(&client.sender, WebSocketMessage {
                                command: CommandType::UpdateConnection {
                                    connection_info: info.clone(),
                                },
                                destination: Destination::Single { destination_uuid: uuid.clone() },
                            }).await;
                        }
                    }
                }
            } else {
                // Received a Connection Info Update from an unknown UUID
            }
        }
        // Client -> Client Messages
        _ => {
            // For anything that doesn't have a specific reply implementation, send it on to the destination directly
            let locked = clients.lock().await;
            match &data.destination {
                Destination::Single { destination_uuid } => {
                    let destination_connection = locked.get(destination_uuid);
                    if let Some(destination_connection) = destination_connection {
                        send_packet(&destination_connection.sender, data).await;
                    }
                }
                _ => {
                    for client in locked.values() {
                        if let Some(connection_info) = &client.client_id {
                            if data.destination.matches_destination(connection_info)
                            {
                                send_packet(&client.sender, data.clone()).await;
                            }
                        }
                    }
                }
            }
        }
    }
}

