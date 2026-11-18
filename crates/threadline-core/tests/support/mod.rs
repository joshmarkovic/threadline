use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header};
use axum::response::Response;
use threadline_core::github::GITHUB_REST_VERSION;
use tokio::net::TcpListener;
use tokio::sync::{Notify, watch};
use tokio::task::JoinHandle;

pub struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
    gate: Option<Arc<Notify>>,
    pub authenticated: bool,
    pub expected_etag: Option<&'static str>,
    pub graph_repository: Option<(&'static str, &'static str)>,
}

impl Reply {
    pub fn new(status: StatusCode, body: impl Into<String>) -> Self {
        Self {
            status,
            headers: HeaderMap::new(),
            body: body.into(),
            gate: None,
            authenticated: false,
            expected_etag: None,
            graph_repository: None,
        }
    }

    pub fn header(mut self, name: HeaderName, value: impl AsRef<str>) -> Self {
        self.headers.insert(
            name,
            HeaderValue::from_str(value.as_ref()).expect("fixture HTTP header"),
        );
        self
    }

    pub fn delayed_until(mut self, gate: Arc<Notify>) -> Self {
        self.gate = Some(gate);
        self
    }
}

#[derive(Debug, serde::Serialize)]
pub struct Observation {
    method: &'static str,
    route: &'static str,
    response_status: u16,
    pub matched: bool,
    pub versioned: bool,
    pub expected_media_type: bool,
    pub authorization_sent: bool,
}

type Route = (Method, String);

#[derive(Default)]
struct Script {
    routes: HashMap<Route, VecDeque<Reply>>,
    observations: Vec<Observation>,
}

struct Shared {
    script: Mutex<Script>,
    request_count: watch::Sender<usize>,
}

pub struct Simulator {
    pub address: SocketAddr,
    shared: Arc<Shared>,
    task: JoinHandle<std::io::Result<()>>,
}

impl Simulator {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test-owned loopback socket");
        let address = listener.local_addr().unwrap();
        let (request_count, _) = watch::channel(0);
        let shared = Arc::new(Shared {
            script: Mutex::default(),
            request_count,
        });
        let router = Router::new()
            .fallback(serve_reply)
            .with_state(shared.clone());
        let task = tokio::spawn(async move { axum::serve(listener, router).await });
        Self {
            address,
            shared,
            task,
        }
    }

    pub fn enqueue(&self, method: Method, path_and_query: &str, reply: Reply) {
        let mut script = self.shared.script.lock().unwrap();
        script
            .routes
            .entry((method, path_and_query.to_owned()))
            .or_default()
            .push_back(reply);
    }

    pub async fn wait_for_requests(&self, count: usize) {
        let mut request_count = self.shared.request_count.subscribe();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            request_count.wait_for(|seen| *seen >= count),
        )
        .await
        .expect("simulator waited for expected request")
        .expect("simulator observations stayed open");
    }

    pub fn take_observations(&self) -> Vec<Observation> {
        let mut script = self.shared.script.lock().unwrap();
        std::mem::take(&mut script.observations)
    }
}

impl Drop for Simulator {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve_reply(State(shared): State<Arc<Shared>>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let path = parts.uri.path_and_query().map_or("/", |u| u.as_str());
    let key = (parts.method.clone(), path.to_owned());
    let graph_body = if parts.method == Method::POST {
        let bytes = axum::body::to_bytes(body, 16 * 1024)
            .await
            .unwrap_or_default();
        serde_json::from_slice::<serde_json::Value>(&bytes).ok()
    } else {
        None
    };
    let reply = {
        let mut script = shared.script.lock().unwrap();
        let reply = script.routes.get_mut(&key).and_then(VecDeque::pop_front);
        let matches = reply.as_ref().is_some_and(|expected| {
            let headers = &parts.headers;
            let authenticated = !expected.authenticated
                || headers
                    .get(header::AUTHORIZATION)
                    .is_some_and(|v| v == "Bearer threadline-test-fixture");
            let expected_tag = headers
                .get(header::IF_NONE_MATCH)
                .and_then(|v| v.to_str().ok())
                == expected.expected_etag;
            let graph = expected.graph_repository.is_none_or(|(owner, name)| {
                let Some(body) = &graph_body else {
                    return false;
                };
                body["variables"] == serde_json::json!({"owner":owner,"name":name})
                    && body["query"].as_str().is_some_and(|q| {
                        q.contains("query RepositoryAccess(") && q.contains("viewerPermission")
                    })
                    && headers
                        .get(header::CONTENT_TYPE)
                        .is_some_and(|v| v == "application/json")
            });
            authenticated
                && expected_tag
                && graph
                && headers
                    .get(header::USER_AGENT)
                    .and_then(|v| v.to_str().ok())
                    .is_some_and(|v| v.starts_with("Threadline/"))
        });
        let route = match parts.uri.path() {
            "/user" => "/user",
            "/user/repos" => "/user/repos",
            "/graphql" => "/graphql",
            p if p.starts_with("/repos/") && p.contains("/issues/") => {
                "/repos/{owner}/{repo}/issues/{number}"
            }
            p if p.starts_with("/repos/") => "/repos/{owner}/{repo}",
            _ => "unrecognized_route",
        };
        script.observations.push(Observation {
            route,
            method: if parts.method == Method::GET {
                "GET"
            } else if parts.method == Method::POST {
                "POST"
            } else {
                "other"
            },
            response_status: reply.as_ref().map_or(400, |r| r.status.as_u16()),
            matched: matches,
            versioned: parts
                .headers
                .get("x-github-api-version")
                .is_some_and(|v| v == GITHUB_REST_VERSION),
            expected_media_type: parts
                .headers
                .get(header::ACCEPT)
                .is_some_and(|v| v == "application/vnd.github+json"),
            authorization_sent: parts.headers.contains_key(header::AUTHORIZATION),
        });
        shared.request_count.send_modify(|count| *count += 1);
        reply
    };
    let Some(reply) = reply else {
        let mut response = Response::new(Body::from("No scripted test response"));
        *response.status_mut() = StatusCode::BAD_REQUEST;
        return response;
    };
    if let Some(gate) = reply.gate {
        gate.notified().await;
    }
    let mut response = Response::new(Body::from(reply.body));
    *response.status_mut() = reply.status;
    *response.headers_mut() = reply.headers;
    response
}
