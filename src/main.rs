use std::net::IpAddr;
use std::str::FromStr;
use std::sync::Arc;
use lock_free::{HashMap};

use samod::storage::TokioFilesystemStorage;
use samod::{
    ConnDirection, DocHandle, DocumentId, Repo
};
use axum::extract::Path;
use axum::{routing::get, Router};
use tokio::net::TcpListener;
use tokio::runtime::Handle;
use tower_http::cors::CorsLayer;

const BAN_DURATION: std::time::Duration = std::time::Duration::from_secs(600);
const MAX_FAILED_ATTEMPTS: i64 = 50;
const CONNECTION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

mod tracing;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    tracing::initialize_tracing();

    // get home directory
    let data_dir = std::env::var("DATA_DIR").unwrap();
    let storage = TokioFilesystemStorage::new(data_dir);

    let repo_handle = Repo::build_tokio()
        .with_storage(storage)
        // .with_announce_policy(move |_, _| {
        //     false
        // })
        .load()
        .await;

    let handle = Handle::current();
    let ip_bans: Arc<HashMap<IpAddr, std::time::Instant>> = Arc::new(HashMap::new());
    let ip_failed_attempts: Arc<HashMap<IpAddr, i64>> = Arc::new(HashMap::new());
    // Start the automerge sync server
    let repo_clone = repo_handle.clone();
    handle.spawn(async move {
        let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
        let addr = format!("0.0.0.0:{}", port);
        let listener = TcpListener::bind(&addr).await.unwrap();

        println!("started automerge sync server on localhost:{}", port);
        // TODO (Samod): Figure out what repo id is
        // println!("repo id: {:?}", repo_clone);

        loop {
            match listener.accept().await {
                Ok((socket, addr)) => {
                    let ip = addr.ip();
                    let banned_at = ip_bans.get(&ip);
                    if banned_at.is_some() {
                        let time_since = std::time::Instant::now() - banned_at.unwrap();
                        if time_since < BAN_DURATION {
                            println!("Client connection rejected, banned for {} more minutes. IP: {ip}", (BAN_DURATION - time_since).as_secs() / 60);
                            continue;
                        } else {
                            ip_bans.remove(&ip);
                        }
                    }
                    println!("Client connected. IP: {ip}");
                    // Handle as automerge connection
                    tokio::spawn({
                        let repo_clone = repo_clone.clone();
                        let ip_bans = ip_bans.clone();
                        let ip_failed_attempts = ip_failed_attempts.clone();
                        async move {
                            let handle_error = || {
                                let failed_attempts: i64 = ip_failed_attempts.get(&ip).unwrap_or(0);
                                if failed_attempts >= MAX_FAILED_ATTEMPTS {
                                    println!("Client has been banned for {} minutes. IP: {ip}", BAN_DURATION.as_secs() / 60);
                                    ip_failed_attempts.insert(ip, 0);
                                    ip_bans.insert(ip, std::time::Instant::now());
                                } else {
                                    ip_failed_attempts.insert(ip, failed_attempts + 1);
                                }
                            };

                            let connection = repo_clone
                                .connect_tokio_io(socket, ConnDirection::Incoming).unwrap();

                            match tokio::time::timeout(CONNECTION_TIMEOUT, connection.handshake_complete()).await
                            {                                
                                Ok(Ok(_)) => {
                                    println!("Client connection completed successfully. IP: {ip}");
                                    // reset failed attempts
                                    ip_failed_attempts.insert(ip, 0);
                                    // remove from banned list
                                    ip_bans.remove(&ip);
                                },
                                Ok(Err(e)) =>{
                                    println!("Client connection error: {:?}. IP: {ip}", e);
                                    handle_error();
                                }
                                Err(_) => {
                                    println!("Client connection timed out. IP: {ip}");
                                    handle_error();
                                }
                            }
                        }
                    });
                }
                Err(e) => println!("couldn't get client: {:?}", e),
            }
        }
    });

    let repo_handle_clone = repo_handle.clone();

    // Start the HTTP server
    let app = Router::new()
        .route(
            "/doc/{id}",
            get(|Path(id): Path<String>| async move {
                println!("Received request for document ID: {}", id);
                match DocumentId::from_str(&id) {
                    Ok(document_id) => {
                        println!("Successfully parsed document ID");
                        match repo_handle_clone.find(document_id).await {
                            Ok(Some(doc_handle)) => {
                                println!("Successfully retrieved document");
                                doc_to_string_full(&doc_handle)
                            }
                            Ok(None) => {
                                let errstr = "Error retrieving document: Not found!".to_string();
                                println!("{}", errstr);
                                errstr
                            }
                            Err(_) => {
                                let errstr = "Error retrieving document: Repo stopped!".to_string();
                                println!("{}", errstr);
                                errstr
                            }
                        }
                    }
                    Err(e) => {
                        println!("Error parsing document ID: {:?}", e);
                        format!("Error parsing document ID: {:?}", e)
                    }
                }
            }),
        )
        .route("/", get(|| async { "fetch documents with /doc/{id}" }))
        .layer(CorsLayer::permissive());

    let http_port = std::env::var("HTTP_PORT").unwrap_or_else(|_| "80".to_string());
    let http_addr = format!("0.0.0.0:{}", http_port);
    println!("starting HTTP server on {}", http_addr);

    let listener = tokio::net::TcpListener::bind(http_addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();

    tokio::signal::ctrl_c().await.unwrap();

    repo_handle.stop().await;
}

fn doc_to_string_full(doc_handle: &DocHandle) -> String {
    let checked_out_doc_json =
        doc_handle.with_document(|d| serde_json::to_string(&automerge::AutoSerde::from(&*d)).unwrap());

    checked_out_doc_json.to_string()
}

#[allow(dead_code)]
fn doc_to_string(doc_handle: &DocHandle) -> String {
    let json_value = doc_handle.with_document(|d| {
        let auto_serde = automerge::AutoSerde::from(&*d);
        serde_json::to_value(&auto_serde).unwrap()
    });

    fn truncate_long_strings(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                for (_, v) in map.iter_mut() {
                    truncate_long_strings(v);
                }
            }
            serde_json::Value::Array(arr) => {
                for v in arr.iter_mut() {
                    truncate_long_strings(v);
                }
            }
            serde_json::Value::String(s) => {
                if s.len() > 50 {
                    *s = format!("{}...", &s[..47]);
                }
            }
            _ => {}
        }
    }

    let mut json_value = json_value;
    truncate_long_strings(&mut json_value);
    serde_json::to_string_pretty(&json_value).unwrap()
}
