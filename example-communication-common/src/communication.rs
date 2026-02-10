use std::cmp::PartialEq;
use std::fmt;
use std::fmt::{Display, Formatter};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConnectionType {
    Client,
    Controller
}

impl PartialEq<ConnectionType> for &ConnectionType {
    fn eq(&self, other: &ConnectionType) -> bool {
        match (self,other) {
            (&ConnectionType::Client, &ConnectionType::Client) => true,
            (&ConnectionType::Controller, &ConnectionType::Controller) => true,
            _ => false
        }
    }
}

impl ConnectionType {
    pub fn as_str(&self) -> &str {
        match self {
            ConnectionType::Client => {"Client"}
            ConnectionType::Controller => {"Controller"}
        }
    }
}
impl Display for ConnectionType {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub uuid: String,
    pub name: String,
    pub connection_type: ConnectionType,
}

impl Display for ConnectionInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "(uuid: {} name: {} connection type: {})", self.uuid, self.name, self.connection_type)
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub enum ControlTypes {
    Message
}

pub struct ControlOption {
    pub display_name: String,
    pub name: String,
    pub default_value: String,
}

pub struct ControlDefinition {
    pub display_name: String,
    pub name: String,
    pub options: Vec<ControlOption>
}

impl ControlTypes {
    pub fn as_str(&self) -> String {
        match self {
            ControlTypes::Message => {"Message".to_string()}
        }
    }
    pub fn to_definition(&self) -> ControlDefinition {
        match self {
            ControlTypes::Message => {
                ControlDefinition {
                    display_name: "Send Message".to_string(),
                    name: self.as_str(),
                    options: vec![ControlOption {
                        display_name: "Text".to_string(),
                        name: "Text".to_string(),
                        default_value: "".to_string(),
                    }],
                }
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub enum ControlMessage {
    Message {
        text: String
    }
}



#[derive(Serialize, Deserialize, Clone)]
pub enum CommandType {
    // Server -> Client
    Welcome {uuid: String},
    ActiveConnections { users: Vec<ConnectionInfo> },
    UpdateConnection { connection_info: ConnectionInfo },
    NotifyDisconnect { uuid: String },
    // Client -> Server
    GetConnections {reply_uuid: String},
    SetConnectionInfo { info: ConnectionInfo },
    Disconnect,
    // Client -> Client
    Control {
        message_type: ControlMessage,
    },
    RequestCapabilities {
        reply_uuid: String
    },
    ProvideCapabilities {
        sender_uuid: String,
        list: Vec<ControlTypes>
    },
    Ack
}

#[derive(Serialize, Deserialize, Clone)]
pub enum Destination {
    Single { destination_uuid: String},
    Multi { destination_uuids: Vec<String>},
    Type { destination_type: ConnectionType },
    All,
    None
}


impl Destination {
    pub fn matches_destination(&self, connection_info: &ConnectionInfo) -> bool {
        match self {
            Destination::Single { destination_uuid } => { *destination_uuid == connection_info.uuid }
            Destination::Multi { destination_uuids } => { destination_uuids.contains(&connection_info.uuid) }
            Destination::Type { destination_type } => { destination_type == connection_info.connection_type }
            Destination::All => { true }
            Destination::None => { false }
        }
    }

    pub fn matches_uuid(&self, uuid: &String) -> bool {
        match self {
            Destination::Single { destination_uuid } => { *destination_uuid == *uuid }
            Destination::Multi { destination_uuids } => { destination_uuids.contains(uuid) }
            Destination::Type { .. } => { false }
            Destination::All => { true }
            Destination::None => { false }
        }

    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct WebSocketMessage {
    pub command: CommandType,
    pub destination: Destination
}
