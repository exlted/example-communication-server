use crate::UIType;
use std::collections::HashMap;
use tokio::sync::mpsc::error::SendError;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::{Notify};
use example_communication_common::{CommandType, ConnectionInfo, ConnectionSettings, ConnectionType, ControlTypes, Destination, FileDefinition, Sender, ThreadSafe, UITypes, WebSocketMessage};
use serde::{Serialize, Deserialize};
use field_name::FieldNames;
use slint::{ModelRc, SharedString, VecModel};
use crate::{ClientCapability, ClientConnection, UIOption};

pub type ThreadSafeSettings = ThreadSafe<MyConfig>;
#[derive(FieldNames, Serialize, Deserialize)]
pub struct MyConfig {
    #[field_name(rename = "name")]
    pub client_name: String,
    pub address: String,
    pub key: String,
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
            client_name: "Controller".to_owned(),
            address: "ws://localhost:8080/ws".to_owned(),
            key: "".to_owned(),
        }
    }
}

impl MyConfig {
    pub fn load() -> MyConfig {
        println!("Loading MyConfig from {:?}", confy::get_configuration_file_path("play_with_me_controller", None));
        let loaded_settings = confy::load("play_with_me_controller", None);

        if loaded_settings.is_err() {
            MyConfig::default()
        }
        else {
            loaded_settings.unwrap()
        }
    }

    pub async fn save(&self) {
        confy::store("play_with_me_controller", None, self).expect("Failed to Store Config");
    }

    pub fn fill_data_model(&self) -> Vec<UIOption> {
        vec!(
            UIOption{
                display: "Client Name".into(),
                name: MyConfig::CLIENT_NAME.into(),
                r#type: UIType::Text,
                value: self.client_name.clone().into(),
                options: ModelRc::new(VecModel::default()),
            },
            UIOption{
                display: "Server URL".into(),
                name: MyConfig::ADDRESS.into(),
                r#type: UIType::Text,
                value: self.address.clone().into(),
                options: ModelRc::new(VecModel::default()),
            },
            UIOption{
                display: "API Key".into(),
                name: MyConfig::KEY.into(),
                r#type: UIType::Text,
                value: self.key.clone().into(),
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
                            uuid: client_cache.lock().await.local_uuid.to_string(),
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
            _ => {

            }
        }
    }
}

pub type ThreadSafeClientCache = ThreadSafe<ClientCache>;
pub struct ClientCache {
    pub local_uuid: String,
    pub to_server: Option<UnboundedSender<WebSocketMessage>>,
    pub connected_clients: Vec<ConnectionInfo>,
    pub client_capabilities: HashMap<String, Vec<ControlTypes>>,
    pub client_files: HashMap<String, HashMap<String, Vec<String>>>,
    pub file_transfer_threads: HashMap<String, UnboundedSender<CommandType>>
}

impl Sender for ClientCache {
    fn get_uuid(&self) -> String {
        self.local_uuid.clone()
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

impl ClientCache {
    pub fn remove_connection(&mut self, uuid: String) {
        self.connected_clients.retain(|c| c.uuid != uuid);
        self.client_capabilities.remove(uuid.as_str());
    }

    pub fn add_or_update_connection(&mut self, connection_info: ConnectionInfo) {
        let found = self.connected_clients.iter_mut().find(|c| c.uuid == connection_info.uuid);

        if let Some(connection) = found {
            connection.name = connection_info.name;
        }
        else {
            self.try_send(WebSocketMessage {
                command: CommandType::RequestCapabilities {
                    reply_uuid: self.local_uuid.clone(),
                },
                destination: Destination::Single{destination_uuid: connection_info.uuid.clone()},
            }).expect("Failed to Send Message");

            self.try_send(WebSocketMessage {
                command: CommandType::AddFileWatch {
                    return_uuid: self.local_uuid.clone()
                },
                destination: Destination::Single{destination_uuid: connection_info.uuid.clone()},
            }).expect("Failed to Send Message");

            self.connected_clients.push(connection_info);
        }
    }

    fn fill_capability_model(&self, uuid: String) -> Vec<ClientCapability> {
        let mut capability_definitions = Vec::new();

        let capabilities = self.client_capabilities.get(&uuid);
        if let Some(capabilities) = capabilities {
            for capability in capabilities {
                let definition = capability.to_definition();

                let mut definition_options = Vec::new();
                for option in definition.options {
                    let ui_type = match option.ui_type {
                        UITypes::Text => {UIType::Text}
                        UITypes::Checkbox => {UIType::Checkbox}
                        UITypes::ComboBox => {UIType::ComboBox}
                    };

                    let mut options: Vec<SharedString> = Vec::new();
                    if option.acceptable_option_types.len() > 0 {
                        if let Some(files) = self.client_files.get(&uuid) {
                            if option.acceptable_option_types[0] == "ALL".to_string() {
                                for file_type in files {
                                    for file in file_type.1 {
                                        options.push(file.clone().into());
                                    }
                                }
                            }
                            else {
                                for file_type in option.acceptable_option_types {
                                    if let Some(files) = files.get(&file_type) {
                                        for file in files {
                                            options.push(file.clone().into());
                                        }
                                    }
                                }
                            }
                        }
                    }

                    definition_options.push(UIOption {
                        display: option.display_name.into(),
                        name: option.name.into(),
                        r#type: ui_type.into(),
                        value: option.default_value.into(),
                        options: ModelRc::new(VecModel::from(options)),
                    });
                }

                capability_definitions.push(
                    ClientCapability {
                        display_name: definition.display_name.into(),
                        name: definition.name.into(),
                        options: ModelRc::new(VecModel::from(definition_options)),
                    }
                );
            }
        }

        capability_definitions
    }

    pub fn fill_data_model(& self) -> Vec<ClientConnection> {
        let mut rv = Vec::new();

        for connected_client in &self.connected_clients {
            let capabilities_model = ModelRc::new(VecModel::from(self.fill_capability_model(connected_client.uuid.clone())));
            rv.push(
                ClientConnection {
                    capabilities: capabilities_model,
                    display_name: connected_client.name.clone().into(),
                    name: connected_client.uuid.clone().into(),
                });
        }

        rv
    }

    pub fn set_files(&mut self, uuid: String, files: Vec<FileDefinition>) {
        let mut new_file_set: HashMap<String, Vec<String>> = HashMap::new();

        for file in files {
            if let Some(file_set) = new_file_set.get_mut(&file.file_type) {
                file_set.push(file.path);
            }
            else {
                new_file_set.insert(file.file_type.clone(), vec![file.path]);
            }
        }

        if self.client_files.contains_key(&uuid) {
            self.client_files.remove(&uuid);
        }

        self.client_files.insert(uuid, new_file_set);
    }

    pub fn update_file(&mut self, uuid: String, file: FileDefinition, add: bool) {
        if let Some(client_files) = self.client_files.get_mut(&uuid) {
            if let Some(file_set) = client_files.get_mut(&file.file_type) {
                if add {
                    file_set.push(file.path);
                }
                else {
                    file_set.retain(|check_file| *check_file != file.path);
                }
            }
        }
        
    }
}