use std::path::Path;
use tokio::sync::mpsc::error::SendError;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::{Mutex, Notify};
use example_communication_common::{CommandType, ConnectionInfo, ConnectionSettings, ConnectionType, Destination, FileDefinition, FileTransfer, Sender, WebSocketMessage};
use serde::{Serialize, Deserialize};
use field_name::FieldNames;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use slint::{ModelRc, VecModel};
use walkdir::WalkDir;
use crate::{UIOption, UIType};

pub type ThreadSafeSettings = Arc<Mutex<MyConfig>>;
#[derive(FieldNames, Serialize, Deserialize)]
pub struct MyConfig {
    #[field_name(rename = "name")]
    pub client_name: String,
    pub address: String,
    pub key: String,
    pub play_sound: bool,
    pub sound_source: String,
    pub accept_file_transfer: bool,
    pub file_transfer_location: String,
}

impl FileTransfer for MyConfig {
    fn get_transfer_location(&self) -> String {
        self.file_transfer_location.clone()
    }
}

impl ConnectionSettings for MyConfig {
    fn get_url(&self) -> String {
        self.address.clone()
    }

    fn get_key(&self) -> String {
        self.key.clone()
    }
}

impl Default for MyConfig {
    fn default() -> Self {
        Self {
            client_name: "Client".to_owned(),
            address: "ws://localhost:8080/ws".to_owned(),
            key: "".to_owned(),
            play_sound: false,
            sound_source: "".to_string(),
            accept_file_transfer: false,
            file_transfer_location: "".to_string(),
        }
    }
}

impl MyConfig {
    pub fn load() -> MyConfig {
        let loaded_settings = confy::load("play_with_me", None);

        if loaded_settings.is_err() {
            MyConfig::default()
        }
        else {
            loaded_settings.unwrap()
        }
    }

    pub async fn save(&self) {
        confy::store("play_with_me", None, self).expect("Failed to Store Config");
    }

    pub fn fill_data_model(&self) -> Vec<UIOption> {
        vec!(
            UIOption {
                display: "Client Name".into(),
                name: MyConfig::CLIENT_NAME.into(),
                r#type: UIType::Text,
                value: self.client_name.clone().into(),
                options: ModelRc::new(VecModel::default()),
            },
            UIOption {
                display: "Server URL".into(),
                name: MyConfig::ADDRESS.into(),
                r#type: UIType::Text,
                value: self.address.clone().into(),
                options: ModelRc::new(VecModel::default()),
            },
            UIOption {
                display: "API Key".into(),
                name: MyConfig::KEY.into(),
                r#type: UIType::Text,
                value: self.key.clone().into(),
                options: ModelRc::new(VecModel::default()),
            },
            UIOption {
                display: "Notification Location".into(),
                name: MyConfig::SOUND_SOURCE.into(),
                r#type: UIType::Text,
                value: self.sound_source.clone().into(),
                options: ModelRc::new(VecModel::default()),
            },
            UIOption {
                display: "File Transfer Destination".into(),
                name: MyConfig::FILE_TRANSFER_LOCATION.into(),
                r#type: UIType::Text,
                value: self.file_transfer_location.clone().into(),
                options: ModelRc::new(VecModel::default()),
            }
        )
    }

    pub async fn on_setting_edited(&mut self, setting: String, new_value: String, client_cache: ThreadSafeClientCache, connection_state_changed: Arc<Notify>) {
        match setting.as_str() {
            MyConfig::CLIENT_NAME => {
                self.client_name = new_value.clone();
                self.save().await;

                client_cache.lock().await.try_send(WebSocketMessage {
                    command: CommandType::UpdateConnection {
                        connection_info: ConnectionInfo {
                            uuid: client_cache.lock().await.uuid.to_string(),
                            name: new_value.to_string(),
                            connection_type: ConnectionType::Client,
                        },
                    },
                    destination: Destination::None,
                }).expect("Failed to Update Connection Information");
            }
            MyConfig::ADDRESS => {
                self.address = new_value.clone();
                self.save().await;
                connection_state_changed.notify_one();
            }
            MyConfig::KEY => {
                self.key = new_value.clone();
                self.save().await;
                connection_state_changed.notify_one();
            }
            MyConfig::SOUND_SOURCE => {
                self.sound_source = new_value.clone();
                self.play_sound = !self.sound_source.is_empty();
                self.save().await;
            }
            MyConfig::FILE_TRANSFER_LOCATION => {
                self.file_transfer_location = new_value.clone();
                client_cache.lock().await.watch_directory(self.file_transfer_location.clone());

                self.accept_file_transfer = !self.file_transfer_location.is_empty();

                self.save().await;
            }
            _ => {

            }
        }
    }
}

pub type ThreadSafeClientCache = Arc<Mutex<ClientCache>>;
pub struct ClientCache {
    pub uuid: String,
    pub to_server: Option<UnboundedSender<WebSocketMessage>>,
    pub(crate) current_directory: String,
    pub file_watcher: RecommendedWatcher,
    pub file_listeners: Vec<String>,
}

impl ClientCache {

    fn build_file_state(&self) -> Vec<FileDefinition> {
        let mut files: Vec<FileDefinition> = Vec::new();

        for entry in WalkDir::new(&self.current_directory).min_depth(1).max_depth(4) {
            if let Ok(entry) = entry {
                if entry.file_type().is_file() {
                    files.push(FileDefinition {
                        path: entry.path().strip_prefix(&self.current_directory).unwrap().to_str().unwrap().to_string(),
                        file_type: entry.path().extension().unwrap().to_str().unwrap().to_string(),
                    })
                }
            }
        }

        files
    }

    fn initialize_file_listener(&self, uuid: String, mut files: Option<Vec<FileDefinition>>) {
        if files.is_none() {
            files = Some(self.build_file_state());
        }

        let files = files.unwrap();

        self.try_send(WebSocketMessage {
            command: CommandType::ProvideFiles {
                uuid: self.uuid.clone(),
                files,
            },
            destination: Destination::Single {
                destination_uuid: uuid,
            },
        }).expect("Failed to ProvideFiles");
    }

    pub fn register_file_listener(&mut self, uuid: String) {
        self.initialize_file_listener(uuid.clone(), None);
        self.file_listeners.push(uuid);
    }
    
    pub fn deregister_file_listener(&mut self, uuid: String) {
        self.file_listeners.retain(|x| *x != uuid);
    }

    pub fn watch_directory(&mut self, path: String) {
        if !self.current_directory.is_empty() {
            self.file_watcher.unwatch(Path::new(&self.current_directory)).unwrap();
        }
        self.current_directory = path;

        self.file_watcher.watch(Path::new(&self.current_directory), RecursiveMode::NonRecursive).unwrap();

        if self.file_listeners.len() > 0 {
            let file_state = self.build_file_state();
            for file_listener in &self.file_listeners {
                self.initialize_file_listener(file_listener.clone(), Some(file_state.clone()));
            }
        }
    }
}

impl Sender for ClientCache {
    fn get_uuid(&self) -> String {
        self.uuid.clone()
    }

    fn try_send(&self, message: WebSocketMessage) -> Result<(), SendError<WebSocketMessage>>{
        if let Some(to_server) = &self.to_server {
            return to_server.send(message)
        }
        Ok(())
    }

    fn drop_connection(&mut self) {
        self.try_send(WebSocketMessage{
            command: CommandType::Disconnect,
            destination: Destination::None
        }).expect("Failed to send message");

        self.to_server = None;
    }

    fn set_connection(&mut self, new_sender: UnboundedSender<WebSocketMessage>) {
        self.to_server = Some(new_sender);
    }
}