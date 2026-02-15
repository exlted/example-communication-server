use std::sync::Arc;
use slint::{spawn_local};
use tokio::select;
use tokio::sync::Notify;
use example_communication_common::{connect_to_server_loop, reconnect, CommandType, ConnectionInfo, ConnectionType, Destination, Sender, Status, WebSocketMessage};
use crate::settings::{ThreadSafeClientCache, ThreadSafeSettings};
use crate::{update_connection_info, UI};

struct ControllerStatus {
    ui: UI,
    running: bool
}

impl Status for ControllerStatus {
    fn update_status(&self, message: String) {
        self.ui.app_window.upgrade_in_event_loop(|ui|{
            ui.set_connection_state(message.into());
        }).expect("Failed to update connection status");
    }
}

pub async fn communication_thread(ui: UI, client_cache: ThreadSafeClientCache, settings: ThreadSafeSettings, connection_state_changed: Arc<Notify>) {
    let mut status = ControllerStatus {
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
async fn handle_message(message: WebSocketMessage, status: &mut ControllerStatus, settings: ThreadSafeSettings, client_cache: ThreadSafeClientCache) {
    match message.command.clone() {
        CommandType::Welcome{ uuid } => {
            let settings = settings.lock().await;
            let mut client_cache = client_cache.lock().await;
            client_cache.local_uuid = uuid;
            client_cache.try_send(WebSocketMessage{
                command: CommandType::SetConnectionInfo {
                    info: ConnectionInfo {
                        uuid: client_cache.local_uuid.to_string(),
                        name: settings.client_name.to_string(),
                        connection_type: ConnectionType::Client,},
                },
                destination: Destination::None,
            }).expect("Failed to send message");
            client_cache.try_send(WebSocketMessage{
                command: CommandType::GetConnections {
                    reply_uuid: client_cache.local_uuid.clone(),
                },
                destination: Destination::None
            }).expect("Failed to send message");
        }

        CommandType::UpdateConnection{ connection_info } =>{
            let mut locked_cache = client_cache.lock().await;
            if connection_info.uuid != locked_cache.local_uuid {
                locked_cache.add_or_update_connection(connection_info);
            }
            
            let client_cache_clone = client_cache.clone();
            status.ui.app_window.upgrade_in_event_loop(|ui| {
                spawn_local(update_connection_info(ui, client_cache_clone)).expect("Failed to update_connection");
            }).expect("Failed to update connection status");
        }

        CommandType::ActiveConnections{ users } => {
            let client_cache_clone = client_cache.clone();
            
            let mut client_cache = client_cache.lock().await;
            for user in users {
                if user.uuid != client_cache.local_uuid {
                    client_cache.add_or_update_connection(user);
                }
            }

            status.ui.app_window.upgrade_in_event_loop(|ui| {
                spawn_local(update_connection_info(ui, client_cache_clone)).expect("Failed to update_connection");
            }).expect("Failed to update connection status");
        }

        CommandType::ProvideCapabilities { sender_uuid, list } => {
            let client_cache_clone = client_cache.clone();
            client_cache.lock().await.client_capabilities.insert(sender_uuid, list);
            status.ui.app_window.upgrade_in_event_loop(|ui| {
                spawn_local(update_connection_info(ui, client_cache_clone)).expect("Failed to update_connection");
            }).expect("Failed to update connection status");
        }

        CommandType::NotifyDisconnect { uuid } => {
            let client_cache_clone = client_cache.clone();
            client_cache.lock().await.remove_connection(uuid);
            status.ui.app_window.upgrade_in_event_loop(|ui| {
                spawn_local(update_connection_info(ui, client_cache_clone)).expect("Failed to update_connection");
            }).expect("Failed to update connection status");
        }

        CommandType::FileTransferAck { name, .. } | CommandType::FileTransferNack { name, .. } => {

            let mut locked_cache = client_cache.lock().await;
            
            if let Some(thread) = locked_cache.file_transfer_threads.get(&name) {
                if !thread.is_closed() {
                    thread.send(message.command).expect("Failed to send message to file transfer thread");
                }
                else {
                    locked_cache.file_transfer_threads.remove(&name);
                }
            }
        }

        CommandType::ProvideFiles { uuid, files} => {
            client_cache.lock().await.set_files(uuid, files);
            let client_cache_clone = client_cache.clone();
            status.ui.app_window.upgrade_in_event_loop(|ui| {
                spawn_local(update_connection_info(ui, client_cache_clone)).expect("Failed to update_connection");
            }).expect("Failed to update connection status");
        }

        CommandType::UpdateFile { uuid, file, add} => {
            client_cache.lock().await.update_file(uuid, file, add);
            let client_cache_clone = client_cache.clone();
            status.ui.app_window.upgrade_in_event_loop(|ui| {
                spawn_local(update_connection_info(ui, client_cache_clone)).expect("Failed to update_connection");
            }).expect("Failed to update connection status");
        }

        _ => {}
    }
}