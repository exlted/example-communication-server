use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use notify::{Event, EventKind};
use slint::{spawn_local, ComponentHandle};
use tokio::select;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::Notify;
use example_communication_common::{connect_to_server_loop, reconnect, CommandType, ConnectionInfo, ConnectionType, ControlMessage, ControlTypes, Destination, FileDefinition, FileTransferClient, Sender, Status, WebSocketMessage};
use crate::commands::spawn_message_box;
use crate::settings::{ThreadSafeClientCache, ThreadSafeSettings};
use crate::{UI};

struct ClientStatus {
    ui: UI,
    running: bool,
    transfers: HashMap<String, FileTransferClient>
}

impl Status for ClientStatus {
    fn update_status(&self, message: String) {
        self.ui.app_window.upgrade_in_event_loop(|ui|{
            ui.set_connection_state(message.into());
        }).expect("Failed to update connection status");
    }
}

pub async fn communication_thread(ui: UI, client_cache: ThreadSafeClientCache, settings: ThreadSafeSettings, connection_state_changed: Arc<Notify>, mut file_watcher_notify: UnboundedReceiver<notify::Result<Event>>) {
    let mut status = ClientStatus{
        ui,
        running: true,
        transfers: HashMap::new()
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
            event = file_watcher_notify.recv() => {
                if let Some(Ok(event)) = event {
                    match event.kind {
                        EventKind::Create(..) => {
                            notify_file_listeners(true, event, client_cache.clone()).await;
                        }
                        EventKind::Remove(..) => {
                            notify_file_listeners(false, event, client_cache.clone()).await;
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

async fn notify_file_listeners(adding: bool, event: Event, client_cache: ThreadSafeClientCache) {
    let client_cache = client_cache.lock().await;
    for path in event.paths {
        for uuid in &client_cache.file_listeners {

            client_cache.try_send(WebSocketMessage {
                command: CommandType::UpdateFile {
                    uuid: client_cache.uuid.clone(),
                    file: FileDefinition {
                        path: path.strip_prefix(client_cache.current_directory.clone()).unwrap().to_str().unwrap().to_string(),
                        file_type: path.extension().unwrap().to_str().unwrap().to_string(),
                    },
                    add: adding,
                },
                destination: Destination::Single { destination_uuid: uuid.clone(),},
            }).expect("Failed to notify listener of file change");
        }
    }

}

async fn handle_message(message: WebSocketMessage, status: &mut ClientStatus, settings: ThreadSafeSettings, client_cache: ThreadSafeClientCache) {

    match message.command.clone() {
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
                    list: vec![ControlTypes::Message, ControlTypes::TransferFile, ControlTypes::DeleteFile],
                },
                destination: Destination::Single{destination_uuid: reply_uuid},
            }).expect("Failed to send message");
        }

        CommandType::StartFileTransfer { name, return_uuid, .. } | CommandType::FileTransferBlob { name, return_uuid, ..} => {
            let return_packets = if let Some(transfer_client) = status.transfers.get_mut(&name) {
                let (return_packets, data_finished) = transfer_client.handle_packet(message.command).await;

                if data_finished {
                    transfer_client.close().await;
                    status.transfers.remove(&name);
                }

                return_packets
            }
            else {
                let mut transfer_client = FileTransferClient::new(name.clone(), settings.clone()).await;
                let (return_packets, data_finished) = transfer_client.handle_packet(message.command).await;


                if data_finished {
                    transfer_client.close().await;
                }
                else {
                    status.transfers.insert(name, transfer_client);
                }

                return_packets
            };

            let locked_cache = client_cache.lock().await;
            for packet in return_packets {
                locked_cache.try_send(WebSocketMessage{
                    command: packet,
                    destination: Destination::Single{
                        destination_uuid: return_uuid.to_string(),
                    },
                }).expect("Failed to send message");
            }
        }

        CommandType::AddFileWatch { return_uuid } => {
            client_cache.lock().await.register_file_listener(return_uuid.to_string());
        }

        CommandType::NotifyDisconnect {uuid} => {
            client_cache.lock().await.deregister_file_listener(uuid.to_string());
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
        ControlMessage::TransferFile => {}
        ControlMessage::DeleteFile {path} => {
            let path = Path::new(&settings.lock().await.file_transfer_location).join(&path);
            std::fs::remove_file(path).expect("Failed to remove file");
        }
    }
}
