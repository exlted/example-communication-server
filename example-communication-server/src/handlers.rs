use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use log::error;
use tokio::sync::Mutex;
use warp::{Filter, Reply};
use crate::client::{Client, ClientMap, Result};
use crate::websocket::client_connection;
use thiserror::Error;
use warp::http::StatusCode;

pub async fn ws_handler(_user: String, ws: warp::ws::Ws, clients: ClientMap) -> Result<impl Reply> {
    println!("ws_handler");

    Ok(ws.on_upgrade(move |socket| client_connection(socket, clients)))
}

pub fn with_clients(clients: Arc<Mutex<HashMap<String, Client>>>) -> impl Filter<Extract = (Arc<Mutex<HashMap<String, Client>>>,), Error = Infallible> + Clone {
    warp::any().map(move || clients.clone())
}

#[derive(Error, Debug)]
pub enum ApiErrors {
    #[error("user not authorized")]
    NotAuthorized(String),
}

pub async fn handle_rejection(err: warp::reject::Rejection) -> std::result::Result<impl Reply, Infallible> {
    let code;
    let message;

    if err.is_not_found() {
        code = StatusCode::NOT_FOUND;
        message = "Not found";
    } else if let Some(_) = err.find::<warp::filters::body::BodyDeserializeError>() {
        code = StatusCode::BAD_REQUEST;
        message = "Invalid Body";
    } else if let Some(e) = err.find::<ApiErrors>() {
        match e {
            ApiErrors::NotAuthorized(_error_message) => {
                code = StatusCode::UNAUTHORIZED;
                message = "Action not authorized";
            }
        }
    } else if let Some(_) = err.find::<warp::reject::MethodNotAllowed>() {
        code = StatusCode::METHOD_NOT_ALLOWED;
        message = "Method not allowed";
    } else {
        // We should have expected this... Just log and say it's a 500
        error!("unhandled rejection: {:?}", err);
        code = StatusCode::INTERNAL_SERVER_ERROR;
        message = "Internal example-communication-server error";
    }

    Ok(warp::reply::with_status(message, code))
}

// ensure that warp`s Reject recognizes `ApiErrors`
impl warp::reject::Reject for ApiErrors {}

pub async fn ensure_authentication() -> impl Filter<Extract = (String,), Error = warp::reject::Rejection> + Clone {
    warp::header::optional::<String>("Authorization").and_then(|auth_header: Option<String>| async move {
        if let Some(header) = auth_header {
            let parts: Vec<&str> = header.split(" ").collect();
            let key = std::env::var("API_KEY").unwrap_or("".to_string());
            if parts.len() == 1 && parts[0] == key {
                return Ok("Existing user".to_string());
            }
        }

        Err(warp::reject::custom(ApiErrors::NotAuthorized(
            "not authorized".to_string(),
        )))
    })
}