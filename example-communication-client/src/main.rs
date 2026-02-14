#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod communication;
mod sound_thread;
mod settings;
mod commands;

use std::sync::Arc;
use notify::{Event};
use communication::communication_thread;
slint::include_modules!();
use slint::{spawn_local, SharedString, Weak};
use tokio::sync::Notify;
use example_communication_common::make_thread_safe;
use crate::settings::{ClientCache, MyConfig, ThreadSafeClientCache, ThreadSafeSettings};

struct UI {
    app_window: Weak<AppWindow>,
}

async fn on_setting_edited(settings: ThreadSafeSettings, setting_name: String, new_value: String, client_cache: ThreadSafeClientCache, connection_state_changed: Arc<Notify>) {
    settings.lock().await.on_setting_edited(setting_name.to_string(), new_value.to_string(), client_cache.clone(), connection_state_changed.clone()).await;
}

#[tokio::main]
pub async fn main() {
    let settings = MyConfig::load();

    let app = AppWindow::new().expect("Failed to spawn UI");

    let options_model = std::rc::Rc::new(slint::VecModel::from(settings.fill_data_model()));
    app.set_options(options_model.clone().into());

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<notify::Result<Event>>();
    let watcher = notify::recommended_watcher(move |result| {tx.send(result).expect("Failed to notify of file watch change.")}).unwrap();

    let mut client_cache = ClientCache{
        uuid: "".to_string(),
        to_server: None,
        current_directory: "".to_string(),
        file_watcher: watcher,
        file_listeners: Vec::new(),
    };

    client_cache.watch_directory(settings.file_transfer_location.clone());

    let settings = make_thread_safe(settings);
    let client_cache = make_thread_safe(client_cache);

    let connection_data_changed = Arc::new(Notify::new());

    let settings_clone = settings.clone();
    let client_cache_clone = client_cache.clone();
    let connection_state_changed_sender = connection_data_changed.clone();
    app.on_option_edited(move |option_name: SharedString, new_value: SharedString| {
        spawn_local(on_setting_edited(settings_clone.clone(), option_name.to_string(), new_value.to_string(), client_cache_clone.clone(), connection_state_changed_sender.clone())).expect("Failed to update settings");
    });

    let ui = UI {
        app_window: app.as_weak(),
    };


    tokio::spawn(communication_thread(ui, client_cache.clone(), settings.clone(), connection_data_changed, rx));

    app.show().expect("Failed to show app window");

    slint::run_event_loop().expect("Failed to run event loop");
}

