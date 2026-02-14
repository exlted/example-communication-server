use std::io::SeekFrom;
use std::path::Path;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use bytes::BytesMut;
use crate::{CommandType, Destination, Sender, ThreadSafe, WebSocketMessage};
use checkasum::hashing::{hash_file_path, HashAlgorithm};

const MAX_ACTIVE_PACKETS: usize = 5;
const BLOB_SIZE: usize = 1024;

pub async fn start_file_transfer(file_path: impl AsRef<Path> + Clone, destination_uuid: String, client_cache: ThreadSafe<impl Sender + Send + Sync + 'static>) -> Option<UnboundedSender<CommandType>>
{
    if !file_path.as_ref().is_file() {
        return None;
    }

    let file = File::open(file_path.clone()).await;
    if file.is_ok() && !destination_uuid.is_empty() {
        // Start transfer thread
        let file = file.unwrap();
        let hash = hash_file_path(&HashAlgorithm::SHA256, file_path.as_ref());
        if hash.is_ok()
        {
            let (sender, receiver) = tokio::sync::mpsc::unbounded_channel::<CommandType>();
            tokio::spawn(file_transfer_loop(file, hash.unwrap(), destination_uuid, file_path.as_ref().file_name().unwrap().to_str().unwrap().to_string(), receiver, client_cache));
            return Some(sender);
        }
    }
    None
}

async fn read_packet(file: &mut File) -> BytesMut {
    let mut buffer = BytesMut::with_capacity(BLOB_SIZE);

    file.read_buf(&mut buffer).await.unwrap();

    buffer
}

async fn file_transfer_loop(mut file: File, checksum: String, destination_uuid: String, file_name: String, mut receiver: UnboundedReceiver<CommandType>, client_cache: ThreadSafe<impl Sender>) {
    let mut active_packets: Vec<CommandType> = Vec::new();
    let filesize = file.metadata().await.unwrap().len();
    let blob_count = filesize / BLOB_SIZE as u64;
    let mut chunk_num = 0;
    let return_uuid = client_cache.lock().await.get_uuid();

    // Generate the file's checksum
    'finish: loop {
        let opening_packet = CommandType::StartFileTransfer {
            name: file_name.clone(),
            chunk_count: blob_count,
            blob_size: BLOB_SIZE,
            checksum: checksum.clone(),
            return_uuid: return_uuid.clone(),
        };

        active_packets.push(opening_packet.clone());

        client_cache.lock().await.try_send(WebSocketMessage {
            command: opening_packet,
            destination: Destination::Single { destination_uuid: destination_uuid.clone() },
        }).expect("Error sending command to the destination");

        loop {

            // fill out active_packets until == MAX_ACTIVE_PACKETS are not Acked
            if file.stream_position().await.unwrap() < filesize {
                while active_packets.len() < MAX_ACTIVE_PACKETS {
                    // generate & send packets
                    let new_packet = CommandType::FileTransferBlob {
                        name: file_name.clone(),
                        chunk_num: chunk_num.clone(),
                        blob: Vec::from(read_packet(&mut file).await),
                        return_uuid: return_uuid.clone(),
                    };
                    chunk_num += 1;

                    active_packets.push(new_packet.clone());

                    client_cache.lock().await.try_send(WebSocketMessage {
                        command: new_packet,
                        destination: Destination::Single { destination_uuid: destination_uuid.clone() },
                    }).expect("Error sending command to the destination");
                }
            }

            // wait for receiver to get Ack and Nack packets to either resend or remove & send new packets to destination
            let command = receiver.recv().await;
            if let Some(command) = command {
                match command {
                    CommandType::FileTransferAck { start, chunk_num, whole, .. } => {
                        if whole {
                            file.seek(SeekFrom::Start(0)).await.unwrap();
                            break 'finish;
                        }

                        if start {
                            active_packets.retain(|command| {
                                match command {
                                    CommandType::StartFileTransfer { .. } => false,
                                    _ => true
                                }
                            })
                        } else {
                            let ack_chunk_num = chunk_num;
                            active_packets.retain(|command| {
                                match command {
                                    CommandType::FileTransferBlob { chunk_num, .. } => {
                                        ack_chunk_num != *chunk_num
                                    }
                                    _ => true
                                }
                            })
                        }
                    }
                    CommandType::FileTransferNack { start, chunk_num, whole, .. } => {
                        if whole {
                            file.seek(SeekFrom::Start(0)).await.unwrap();
                            continue 'finish;
                        }
                        'local: for packet in active_packets.iter() {
                            let ack_chunk_num = chunk_num;
                            match packet {
                                CommandType::StartFileTransfer { .. } => {
                                    if start {
                                        client_cache.lock().await.try_send(WebSocketMessage {
                                            command: packet.clone(),
                                            destination: Destination::Single { destination_uuid: destination_uuid.clone() },
                                        }).expect("Error sending command to the destination");

                                        break 'local
                                    }
                                }
                                CommandType::FileTransferBlob { chunk_num, .. } => {
                                    if *chunk_num == ack_chunk_num {
                                        client_cache.lock().await.try_send(WebSocketMessage {
                                            command: packet.clone(),
                                            destination: Destination::Single { destination_uuid: destination_uuid.clone() },
                                        }).expect("Error sending command to the destination");

                                        break 'local
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}