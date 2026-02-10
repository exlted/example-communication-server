use std::sync::Arc;
use slint::{spawn_local, ComponentHandle};
use tokio::select;
use tokio::sync::Notify;
use example_communication_common::{connect_to_server_loop, reconnect, CommandType, ConnectionInfo, ConnectionType, ControlMessage, ControlTypes, Destination, Status, WebSocketMessage};
use crate::commands::spawn_message_box;
use crate::settings::{ThreadSafeClientCache, ThreadSafeSettings};
use crate::{UI};

struct ClientStatus {
    ui: UI,
    running: bool
}

impl Status for ClientStatus {
    fn update_status(&self, message: String) {
        self.ui.app_window.upgrade_in_event_loop(|ui|{
            ui.set_connection_state(message.into());
        }).expect("Failed to update connection status");
    }
}

pub async fn communication_thread(ui: UI, client_cache: ThreadSafeClientCache, settings: ThreadSafeSettings, connection_state_changed: Arc<Notify>) {
    let mut status = ClientStatus{
        ui,
        running: true,
    };

    let (to_server, mut from_server) = connect_to_server_loop(settings.clone(), connection_state_changed.clone(), &status).await;

    client_cache.lock().await.to_server = Some(to_server);

    while status.running {
        select! {
            message = from_server.recv() => {
                if let Some(message) = message {
                    handle_message(message, &mut status, settings.clone(), client_cache.clone()).await;
                }
            },
            _ = connection_state_changed.notified() => {
                from_server = reconnect(client_cache.clone(), settings.clone(), from_server, connection_state_changed.clone(), &status).await;
            }
        }
    }
}

async fn handle_message(message: WebSocketMessage, status: &mut ClientStatus, settings: ThreadSafeSettings, client_cache: ThreadSafeClientCache) {

    match message.command {
        CommandType::Welcome{ uuid } => {
            println!("Server assigned ID of: {}" ,uuid);
            let settings = settings.lock().await;
            let mut client_cache = client_cache.lock().await;
            client_cache.uuid = uuid;
            client_cache.try_send(WebSocketMessage{
                command: CommandType::SetConnectionInfo {
                    info: ConnectionInfo {
                        uuid: client_cache.uuid.to_string(),
                        name: settings.client_name.to_string(),
                        connection_type: ConnectionType::Client,},
                },
                destination: Destination::None,
            }).expect("Failed to send message");
        }
        
        CommandType::Control{ message_type } => {
            handle_control_message(message_type, status, settings.clone(), client_cache).await;
        }
        
        CommandType::RequestCapabilities { reply_uuid } => {
            let locked_cache = client_cache.lock().await;
            locked_cache.try_send(WebSocketMessage {
                command: CommandType::ProvideCapabilities {
                    sender_uuid: locked_cache.uuid.clone(),
                    list: vec![ControlTypes::Message],
                },
                destination: Destination::Single{destination_uuid: reply_uuid},
            }).expect("Failed to send message");
        }
        
        _ => {}
    }
}

async fn handle_control_message(message: ControlMessage, status: &mut ClientStatus, settings: ThreadSafeSettings, _client_cache: ThreadSafeClientCache) {
    match message {
        ControlMessage::Message { text } => {
            status.ui.app_window.upgrade_in_event_loop(move |ui| {
                spawn_local(spawn_message_box(text, ui.as_weak(), settings.clone())).expect("Failed to spawn message box");
            }).expect("Failed to spawn message box");
        }
    }
}
