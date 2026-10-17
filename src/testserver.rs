//! A local stand-in for Sentry: answers every request the same way and
//! remembers what it was asked. Nothing in the tests reaches the real network.

use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

#[derive(Debug, Clone)]
pub struct Seen {
    pub url: String,
    pub authorization: String,
}

pub struct TestServer {
    server: Arc<tiny_http::Server>,
    seen: Arc<Mutex<Vec<Seen>>>,
    thread: Option<JoinHandle<()>>,
}

impl TestServer {
    pub fn answering(status: u16, body: &str) -> TestServer {
        let routes = vec![(String::new(), status, body.to_string())];
        TestServer::routing(routes)
    }

    /// Answers by the first route whose text is part of the URL; the empty text
    /// matches anything.
    pub fn routing(routes: Vec<(String, u16, String)>) -> TestServer {
        let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));

        let thread = {
            let (server, seen) = (server.clone(), seen.clone());
            std::thread::spawn(move || {
                for request in server.incoming_requests() {
                    let url = request.url().to_string();
                    let authorization = request
                        .headers()
                        .iter()
                        .find(|header| header.field.equiv("Authorization"))
                        .map(|header| header.value.to_string())
                        .unwrap_or_default();
                    seen.lock().unwrap().push(Seen {
                        url: url.clone(),
                        authorization,
                    });

                    let (status, body) = routes
                        .iter()
                        .find(|(part, _, _)| url.contains(part.as_str()))
                        .map(|(_, status, body)| (*status, body.clone()))
                        .unwrap_or((404, String::new()));
                    let _ = request
                        .respond(tiny_http::Response::from_string(body).with_status_code(status));
                }
            })
        };

        TestServer {
            server,
            seen,
            thread: Some(thread),
        }
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.server.server_addr().to_ip().unwrap())
    }

    pub fn requests(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
