#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::path::{Path};
use std::sync::Arc;
use slint::{spawn_local, Model, ModelRc, SharedString, VecModel, Weak};
use tokio::sync::Notify;
use example_communication_common::{Destination, Sender, make_thread_safe, CommandType, ControlMessage, WebSocketMessage, start_file_transfer};
use crate::communication::communication_thread;
use crate::settings::{ClientCache, MyConfig, ThreadSafeClientCache, ThreadSafeSettings};

mod settings;
mod communication;

slint::include_modules!();

struct UI {
    app_window: Weak<AppWindow>,
}

async fn on_setting_edited(settings: ThreadSafeSettings, setting_name: String, new_value: String, client_cache: ThreadSafeClientCache, connection_state_changed: Arc<Notify>) {
    settings.lock().await.on_setting_edited(setting_name.to_string(), new_value.to_string(), client_cache.clone(), connection_state_changed.clone()).await;
}

pub async fn update_connection_info(app: AppWindow, client_cache: ThreadSafeClientCache) {
    let connections_model = ModelRc::new(VecModel::from(client_cache.lock().await.fill_data_model()));
    app.set_connections(connections_model);
}

pub async fn run_command(client_cache: ThreadSafeClientCache, destination_uuid: SharedString, command_name: SharedString, options: ModelRc<UIOption>) {
    let vec_options = options.as_any().downcast_ref::<VecModel<UIOption>>().expect("We know we set a VecModel earlier");
    let mut hashed_options: HashMap<String, UIOption> = HashMap::new();

    for option in vec_options.iter() {
        hashed_options.insert(option.name.to_string(), option.clone());
    }

    let command = match command_name.as_str() {
        "Message" => {
            CommandType::Control {
                message_type: ControlMessage::Message {
                    text: hashed_options["Text"].value.to_string(),
                }
            }
        }
        "TransferFile" => {
            let sender = start_file_transfer(hashed_options["File"].value.to_string(), destination_uuid.to_string().clone(), client_cache.clone()).await;

            if let Some(sender) = sender {
                client_cache.lock().await.file_transfer_threads.insert(Path::new(&hashed_options["File"].value.to_string()).file_name().unwrap().to_str().unwrap().to_string(), sender);
            }

            CommandType::Ack {}
        }
        "DeleteFile" => {
            CommandType::Control {
                message_type: ControlMessage::DeleteFile {
                    path: hashed_options["File"].value.to_string(),
                }
            }
        }
        _ => {
            CommandType::Ack {}
        }
    };

    let _ = client_cache.lock().await.try_send(WebSocketMessage{
        command,
        destination: Destination::Single{destination_uuid: destination_uuid.to_string()},
    });
}

#[tokio::main]
async fn main() {
    let settings = MyConfig::load();

    let app = AppWindow::new().expect("Failed to spawn UI");

    let options_model = ModelRc::new(VecModel::from(settings.fill_data_model()));
    app.set_options(options_model.clone().into());

    let client_cache = make_thread_safe(ClientCache{
        local_uuid: "".to_string(),
        to_server: None,
        connected_clients: Vec::new(),
        client_capabilities: HashMap::new(),
        file_transfer_threads: HashMap::new(),
        client_files: HashMap::new(),
    });
    let settings = make_thread_safe(settings);

    let connection_data_changed = Arc::new(Notify::new());

    let settings_clone = settings.clone();
    let client_cache_clone = client_cache.clone();
    let connection_state_changed_sender = connection_data_changed.clone();
    app.on_option_edited(move |option_name: SharedString, new_value: SharedString| {
        spawn_local(on_setting_edited(settings_clone.clone(), option_name.to_string(), new_value.to_string(), client_cache_clone.clone(), connection_state_changed_sender.clone())).expect("Failed to update settings");
    });

    let client_cache_clone = client_cache.clone();
    app.on_capability_ran(move |client_name, capability_name, selected_options| {
        spawn_local(run_command(client_cache_clone.clone(), client_name, capability_name, selected_options)).expect("Failed to Run Command");
    });

    let ui = UI {
        app_window: app.as_weak(),
    };

    tokio::spawn(communication_thread(ui, client_cache.clone(), settings.clone(), connection_data_changed));

    app.show().expect("Failed to show app window");

    slint::run_event_loop().expect("Failed to run event loop");
}
