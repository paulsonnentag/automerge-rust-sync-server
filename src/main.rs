use std::str::FromStr;

use automerge_repo::share_policy::ShareDecision;
use automerge_repo::tokio::FsStorage;
use automerge_repo::{
    ConnDirection, DocHandle, DocumentId, Repo, RepoId, SharePolicy, SharePolicyError,
};
use axum::extract::Path;
use axum::{routing::get, Router};
use futures::future::BoxFuture;
use tokio::net::TcpListener;
use tokio::runtime::Handle;
use tower_http::cors::CorsLayer;
use tracing_subscriber;

pub struct Restrictive;

impl SharePolicy for Restrictive {
    fn should_sync(
        &self,
        _document_id: &DocumentId,
        _with_peer: &RepoId,
    ) -> BoxFuture<'static, Result<ShareDecision, SharePolicyError>> {
        Box::pin(async move { Ok(ShareDecision::Share) })
    }

    fn should_request(
        &self,
        _document_id: &DocumentId,
        _from_peer: &RepoId,
    ) -> BoxFuture<'static, Result<ShareDecision, SharePolicyError>> {
        Box::pin(async move { Ok(ShareDecision::Share) })
    }

    fn should_announce(
        &self,
        _document_id: &DocumentId,
        _to_peer: &RepoId,
    ) -> BoxFuture<'static, Result<ShareDecision, SharePolicyError>> {
        Box::pin(async move { Ok(ShareDecision::DontShare) })
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    tracing_subscriber::fmt::init();
    // get home directory
    let data_dir = std::env::var("DATA_DIR").unwrap();
    let storage = FsStorage::open(data_dir).unwrap();
    let repo = Repo::new(Some("sync-server".to_string()), Box::new(storage));
    let repo_handle = repo.run();

    let handle = Handle::current();

    // Start the automerge sync server
    let repo_clone = repo_handle.clone();
    handle.spawn(async move {
        let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
        let addr = format!("0.0.0.0:{}", port);
        let listener = TcpListener::bind(&addr).await.unwrap();

        println!("started automerge sync server on localhost:{}", port);
        println!("repo id: {:?}", repo_clone.get_repo_id().clone());

        loop {
            match listener.accept().await {
                Ok((mut socket, addr)) => {
                    let ip = addr.ip();
                    println!("Client connected. IP: {ip}");
                    // Handle as automerge connection
                    tokio::spawn({
                        let repo_clone = repo_clone.clone();
                        async move {
                            match repo_clone
                                .connect_tokio_io(addr, socket, ConnDirection::Incoming)
                                .await
                            {
                                Ok(_) => println!("Client connection completed successfully. IP: {ip}"),
                                Err(e) => println!("Client connection error: {:?}. IP: {ip}", e),
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
                        match repo_handle_clone.request_document(document_id).await {
                            Ok(doc_handle) => {
                                println!("Successfully retrieved document");
                                doc_to_string_full(&doc_handle)
                            }
                            Err(e) => {
                                println!("Error retrieving document: {:?}", e);
                                format!("Error retrieving document: {:?}", e)
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

    repo_handle.stop().unwrap();
}

fn doc_to_string_full(doc_handle: &DocHandle) -> String {
    let checked_out_doc_json =
        doc_handle.with_doc(|d| serde_json::to_string(&automerge::AutoSerde::from(d)).unwrap());

    checked_out_doc_json.to_string()
}

fn doc_to_string(doc_handle: &DocHandle) -> String {
    let json_value = doc_handle.with_doc(|d| {
        let auto_serde = automerge::AutoSerde::from(d);
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
