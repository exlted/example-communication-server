use tokio::sync::mpsc::error::SendError;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::{Mutex, Notify};
use example_communication_common::{CommandType, ConnectionInfo, ConnectionSettings, ConnectionType, Destination, Sender, WebSocketMessage};
use serde::{Serialize, Deserialize};
use field_name::FieldNames;
use crate::UIOption;

pub type ThreadSafeSettings = Arc<Mutex<MyConfig>>;
#[derive(FieldNames, Serialize, Deserialize)]
pub struct MyConfig {
    #[field_name(rename = "name")]
    pub client_name: String,
    pub address: String,
    pub key: String,
    pub play_sound: bool,
    pub sound_source: String,
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
                value: self.client_name.clone().into(),
            },
            UIOption {
                display: "Server URL".into(),
                name: MyConfig::ADDRESS.into(),
                value: self.address.clone().into(),
            },
            UIOption {
                display: "API Key".into(),
                name: MyConfig::KEY.into(),
                value: self.key.clone().into(),
            },
            UIOption {
                display: "Notification Location".into(),
                name: MyConfig::SOUND_SOURCE.into(),
                value: self.sound_source.clone().into(),
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
            _ => {

            }
        }
    }
}

pub type ThreadSafeClientCache = Arc<Mutex<ClientCache>>;
pub struct ClientCache {
    pub uuid: String,
    pub to_server: Option<UnboundedSender<WebSocketMessage>>
}

impl ClientCache {
    pub fn try_send(&self, message: WebSocketMessage) -> Result<(), SendError<WebSocketMessage>> {
        if let Some(to_server) = &self.to_server {
            return to_server.send(message)
        }
        Ok(())
    }
}

impl Sender for ClientCache {
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