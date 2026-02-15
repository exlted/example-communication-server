use std::path::{Path, PathBuf};
use checkasum::hashing::{hash_file_path, hash_matches, HashAlgorithm};
use tokio::fs::{create_dir_all, remove_file, File};
use tokio::io::AsyncWriteExt;
use crate::{CommandType, ThreadSafe};

pub struct FileTransferClient {
    pub name: String,

    file_path: PathBuf,
    file: File,
    // For packets seen out-of-order so we can hold them until we're ready to write them to the file
    cached_packets: Vec<CommandType>,
    last_printed_packet: i32,
    packet_count: u64,
    expected_blob_size: usize,
    checksum: String,
}

pub trait FileTransfer {
    fn get_transfer_location(&self) -> String;
}

impl FileTransferClient {
    pub async fn new(name: String, settings: ThreadSafe<impl FileTransfer>) -> Self {
        let destination = settings.lock().await.get_transfer_location();
        let path = Path::new(&destination).join(&name);

        create_dir_all(path.parent().unwrap()).await.unwrap();
        let file = File::create(path.clone()).await.unwrap();

        Self {name, file_path: path, file, cached_packets: vec![], last_printed_packet: -1, packet_count: 0, expected_blob_size: 0, checksum: String::new()}
    }

    pub async fn close(&mut self) {
        self.file.flush().await.unwrap();
    }

    pub async fn handle_packet(&mut self, new_packet: CommandType) -> (Vec<CommandType>, bool) {
        let mut return_packets: Vec<CommandType> = vec![];

        match new_packet.clone() {
            CommandType::StartFileTransfer { chunk_count, blob_size, checksum, .. } => {
                self.packet_count = chunk_count;
                self.expected_blob_size = blob_size;
                self.checksum = checksum;

                return_packets.push(CommandType::FileTransferAck {
                    name: self.name.clone(),
                    start: true,
                    chunk_num: 0,
                    whole: false,
                });
            }
            CommandType::FileTransferBlob { chunk_num, blob, .. } => {

                if self.checksum.is_empty() {
                    // Got a Blob first, Nack the Start
                    return_packets.push(CommandType::FileTransferNack {
                        name: self.name.clone(),
                        start: true,
                        chunk_num: 0,
                        whole: false,
                    });

                    // Additionally, Nack any missed packets up until this point
                    for n in self.last_printed_packet + 1 .. chunk_num {
                        return_packets.push(CommandType::FileTransferNack {
                            name: self.name.clone(),
                            start: false,
                            chunk_num: n,
                            whole: false,
                        });
                    }

                    self.cached_packets.push(new_packet);
                }
                else if self.last_printed_packet + 1 == chunk_num {
                    self.file.write_all(&blob).await.unwrap();
                    return_packets.push(CommandType::FileTransferAck {
                        name: self.name.clone(),
                        start: false,
                        chunk_num,
                        whole: false,
                    });
                    self.last_printed_packet = chunk_num;

                    for packet in &self.cached_packets {
                        match packet {
                            CommandType::FileTransferBlob {chunk_num, blob, ..} => {
                                if *chunk_num == self.last_printed_packet + 1 {
                                    self.file.write_all(&blob).await.unwrap();

                                    self.last_printed_packet += 1;
                                    continue;
                                }
                            }
                            _ => {

                            }
                        }
                    }

                    self.cached_packets.retain(|packet| {
                        return match packet {
                            CommandType::FileTransferBlob { chunk_num, .. } => {
                                *chunk_num > self.last_printed_packet
                            }
                            _ => {
                                false
                            }
                        }
                    });
                }
                else {
                    self.cached_packets.push(new_packet);
                    self.cached_packets.sort_by(|a, b| {
                        match (a, b) {
                            (CommandType::FileTransferBlob { chunk_num: a_num, .. }, CommandType::FileTransferBlob { chunk_num: b_num, ..}) => {
                                a_num.cmp(&b_num)
                            }
                            (CommandType::FileTransferBlob { .. }, _) => {
                                core::cmp::Ordering::Greater
                            }
                            (_,_) => {
                                core::cmp::Ordering::Equal
                            }
                        }
                    });

                    for n in self.last_printed_packet + 1 .. chunk_num {
                        return_packets.push(CommandType::FileTransferNack {
                            name: self.name.clone(),
                            start: false,
                            chunk_num: n,
                            whole: false,
                        });
                    }

                    return_packets.push(CommandType::FileTransferAck {
                        name: self.name.clone(),
                        start: false,
                        chunk_num,
                        whole: false,
                    })
                }

            }
            _ => {}
        }

        if self.last_printed_packet >= self.packet_count as i32 {
            let hash = hash_file_path(&HashAlgorithm::SHA256, &*self.file_path);
            if let Ok(hash) = hash {
                if !hash_matches(&*hash, self.checksum.as_str()) {
                    // File was not successfully transferred, Nack the whole file, delete it and have the sender restart
                    return_packets.push(CommandType::FileTransferNack {
                        name: self.name.clone(),
                        start: false,
                        chunk_num: 0,
                        whole: true,
                    });
                    self.file.flush().await.unwrap();
                    remove_file(self.file_path.clone()).await.unwrap();
                }
                else {

                    return_packets.push(CommandType::FileTransferAck {
                        name: self.name.clone(),
                        start: false,
                        chunk_num: 0,
                        whole: true,
                    });
                }
            }
        }

        (return_packets, self.last_printed_packet >= self.packet_count as i32)
    }
}