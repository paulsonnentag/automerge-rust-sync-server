use std::str::FromStr;

use automerge_repo::tokio::FsStorage;
use automerge_repo::{ConnDirection, DocHandle, DocumentId, Repo};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::runtime::Handle;
use tracing_subscriber;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    tracing_subscriber::fmt::init();

    let storage = FsStorage::open("/tmp/automerge-server-data").unwrap();
    let repo = Repo::new(Some("sync-server".to_string()), Box::new(storage));
    let repo_handle = repo.run();

    let handle = Handle::current();


    let repo_clone = repo_handle.clone();
    handle.spawn(async move {
        let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
        let addr = format!("0.0.0.0:{}", port);
        let listener = TcpListener::bind(&addr).await.unwrap();

        println!("started server on localhost:{}", port);
        println!("repo id: {:?}", repo_clone.get_repo_id().clone());

        loop {
            match listener.accept().await {
                Ok((mut socket, addr)) => {
                    println!("client connected");

                    // Read first few bytes to check if it's HTTP
                    let mut buf = [0; 4];
                    match socket.peek(&mut buf).await {
                        Ok(_) => {
                            if buf.starts_with(b"GET ") || buf.starts_with(b"POST") {
                                // It's an HTTP request, send 200 OK

                                // Extract the path from the HTTP request
                                let mut path_buf = [0; 1024];
                                let path = if let Ok(n) = socket.peek(&mut path_buf).await {
                                    let request = String::from_utf8_lossy(&path_buf[..n]);
                                    request.lines()
                                        .next()
                                        .and_then(|path_line| path_line.split_whitespace().nth(1))
                                        .map(|p| p.to_string())
                                } else {
                                    None
                                };

                                let content = if let Some(ref path) = path {
                                  println!("path: {}", path);
                                  
                                  
                                  let parts: Vec<&str> = path[1..].split('/').collect();
                                  match parts.get(0) {
                                    Some(id_str) => {
                                      match DocumentId::from_str(id_str) {
                                        Ok(id) => {

                                          match repo_clone.request_document(id).await {
                                            Ok(doc) => {
                                              if parts.get(1) == Some(&"full") {
                                                doc_to_string_full(&doc)
                                              } else {
                                                doc_to_string(&doc)
                                              }
                                            },
                                            Err(e) => String::from(format!("error: {:?}", e)),
                                          }
                                        }
                                        Err(e) => {

                                            let repo_id = repo_clone.get_repo_id().clone();
                                            println!("error: {:?}", e);
                                          String::from(
                                              format!("Repo id: {:?}\n\n\
                                              Usage:\n\
                                              - /:automergeId         - Get document content with truncated strings\n\
                                              - /:automergeId/full    - Get complete document", repo_id)
                                          )
                                        }
                                      }
                                    },
                                    None => {
                                      String::from("error")                                    
                                    }
                                  }
                                } else {
                                  String::from("error")
                                };


                                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}", content.len(), content);

                                let _ = socket.write_all(response.as_bytes()).await;
                                continue;
                            }
                        }
                        Err(e) => {
                            println!("Error peeking socket: {:?}", e);
                            continue;
                        }
                    }

                    // Not HTTP, handle as automerge connection
                    tokio::spawn({
                        let repo_clone = repo_clone.clone();
                        async move {
                            match repo_clone
                                .connect_tokio_io(addr, socket, ConnDirection::Incoming)
                                .await
                            {
                                Ok(_) => println!("Client connection completed successfully"),
                                Err(e) => println!("Client connection error: {:?}", e),
                            }
                        }
                    });
                }
                Err(e) => println!("couldn't get client: {:?}", e),
            }
        }
    });

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