use std::error::Error;
// use std::io::{BufRead, BufReader, Write};
use std::sync::Arc;
use std::sync::mpsc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Semaphore;

use crate::task::Task;
use crate::task::TaskType;

pub trait ServerTrait {
    async fn start_server(
        &self,
        address: String,
        tx: mpsc::Sender<Result<(), Box<dyn Error + Send>>>,
    );
}

pub struct Server;

impl ServerTrait for Server {
    async fn start_server(
        &self,
        address: String,
        tx: mpsc::Sender<Result<(), Box<dyn Error + Send>>>,
    ) {
        println!("Starting the server");
        let listener = tokio::net::TcpListener::bind(address).await;

        match listener {
            Ok(_) => tx.send(Ok(())).unwrap(),
            Err(e) => {
                println!("here {}", e);
                tx.send(Err(Box::new(e))).unwrap();
                return;
            }
        }
        let listener = listener.unwrap();

        let cpu_semaphore = tokio::sync::Semaphore::new(40);
        let cpu_semaphore = std::sync::Arc::new(cpu_semaphore);

        let _ = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let cpu_semaphore_clone = cpu_semaphore.clone();
                        let _ = tokio::spawn(async move {
                            Self::handle_connection(stream, cpu_semaphore_clone).await;
                        });
                    }
                    Err(e) => {
                        eprintln!("Error accepting connection: {}", e);
                    }
                }
            }
        })
        .await;
    }
}

impl Server {
    async fn handle_connection(stream: TcpStream, cpu_semaphore: Arc<Semaphore>) {
        let (reader, mut writer) = stream.into_split();
        let mut reader = tokio::io::BufReader::new(reader);

        let (tx, rx) = mpsc::channel::<u8>();
        let (drop_client_tx, drop_client_rx) = mpsc::channel::<u8>();

        tokio::spawn(async move {
            while let Ok(message) = rx.recv() {
                writer.write(&[message]).await.unwrap();
            }
        });

        tokio::spawn(async move {
            loop {
                let mut line = String::new();

                match reader.read_line(&mut line).await {
                    Ok(0) => {
                        return;
                    }
                    Ok(_) => {
                        let tx_clone = tx.clone();
                        let drop_client_tx_clone = drop_client_tx.clone();
                        let cpu_semaphore_clone = cpu_semaphore.clone();
                        tokio::spawn(async move {
                            let response = Self::get_task_value(line, cpu_semaphore_clone).await;
                            if let Some(r) = response {
                                tx_clone.send(r).unwrap();
                            } else {
                                drop_client_tx_clone.send(0).unwrap();
                                return;
                            }
                        });
                    }
                    Err(e) => {
                        eprintln!("Unable to get command due to: {}", e);
                        return;
                    }
                }

                if let Ok(0) = drop_client_rx.try_recv() {
                    return;
                }
            }
        });
    }

    async fn get_task_value(buf: String, cpu_semaphore: Arc<Semaphore>) -> Option<u8> {
        let numbers: Vec<&str> = buf.trim().split(':').collect();
        let task_number = numbers.first().unwrap().parse::<u8>().ok()?;
        let seed = numbers.last().unwrap().parse::<u64>().ok()?;

        match TaskType::from_u8(task_number) {
            Some(task_type) => match task_type {
                TaskType::CpuIntensiveTask => {
                    let _permit = cpu_semaphore.acquire_owned().await.unwrap();
                    tokio::task::spawn_blocking(move || {
                        Task::execute(task_number, seed) // assumes a sync version exists
                    })
                    .await
                    .ok()
                }
                _ => Some(Task::execute_async(task_number, seed).await),
            },
            None => None,
        }
    }
}
