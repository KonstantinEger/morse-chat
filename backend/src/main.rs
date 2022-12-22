use std::collections::HashMap;
use std::sync::Arc;

use backend::HeaderName;
use tokio::net::{TcpListener, TcpStream};
use sha1::{Sha1, Digest};

use backend::request::{Method, Request};
use backend::response::{Response, Status};
use tokio::sync::Mutex;
use tokio::task::{self, JoinHandle};
use websockets::WebSocket;

#[derive(Default)]
struct AppData {
    sockets: HashMap<usize, WebSocket>,
}

type SharedAppData = Arc<Mutex<AppData>>;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let server = TcpListener::bind(("127.0.0.1", 8080)).await?;
    let app_data: SharedAppData = Arc::new(Mutex::new(Default::default()));

    let _listener_task = task::spawn(msg_listener_task(Arc::clone(&app_data)));

    loop {
        let (mut stream, _) = if let Ok(s) = server.accept().await {
            s
        } else {
            continue;
        };
        let request = if let Ok(req) = Request::try_parse_from(&mut stream).await {
            req
        } else {
            let response = Response::builder()
                .with_status(Status::BadRequest)
                .with_body(Vec::new());
            let _ = response.try_write_to(&mut stream).await;
            continue;
        };
        let _ = handle(request, stream, Arc::clone(&app_data)).await;
    }
}

async fn msg_listener_task(app_data: SharedAppData) {
    println!("task started");
    loop {
        let mut delete_ids = Vec::new();
        let mut lock = app_data.lock().await;
        for (&id, socket) in &lock.sockets {
            let msg = socket.poll_next_message().await;
            match msg {
                Some(Err(e)) => {
                    dbg!(e);
                    delete_ids.push(id);
                },
                Some(Ok(msg)) => {
                    dbg!(id, msg);
                },
                _ => {}
            }
        }
        for id in delete_ids {
            lock.sockets.remove(&id);
        }
        drop(lock);
        tokio::time::sleep(std::time::Duration::from_millis(16)).await;
    }
}

async fn handle(
    req: Request,
    mut stream: TcpStream,
    app_data: SharedAppData
) -> anyhow::Result<()> {
    let mut upgraded_to_ws = false;
    let response: Response = match (req.method(), req.path()) {
        (Method::Get, "/") | (Method::Get, "/index.html") => {
            // serve index html
            let html = include_str!("../../frontend/index.html");
            Response::builder()
                .as_html()
                .with_body(html)
        },
        (Method::Get, "/ws") if fulfills_ws_requirements(&req) => {
            // upgrade to websocket
            upgraded_to_ws = true;
            let nonce = req.headers().get(&HeaderName::from_str("sec-websocket-key")).unwrap();
            let hash = get_websocket_accept_hash(nonce);
            Response::builder()
                .with_status(Status::SwitchingProtocols)
                .with_header("connection", "Upgrade")
                .with_header("upgrade", "websocket")
                .with_header("sec-websocket-accept", hash)
                .with_body(Vec::new())
        },
        (_, path) => {
            Response::builder()
                .with_status(Status::NotFound)
                .with_body(format!("Error 404: no resource with path {} found", path))
        }
    };

    response.try_write_to(&mut stream).await?;

    if upgraded_to_ws {
        let ws = WebSocket::new(stream);
        let mut lock = app_data.lock().await;
        let id = lock.sockets.len();
        lock.sockets.insert(id, ws);
    }

    Ok(())
}

fn get_websocket_accept_hash(nonce: &str) -> String {
    let concat = String::from(nonce) + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
    let mut hasher = Sha1::new();
    hasher.update(concat);
    let result = hasher.finalize();
    base64::encode(result.as_slice())
}

fn fulfills_ws_requirements(req: &Request) -> bool {
    req.headers()
        .get(&HeaderName::from_str("connection"))
        .map(|v| v.to_ascii_lowercase() == "upgrade")
        .and_then(|has_conn| Some(has_conn && req.headers()
                  .get(&HeaderName::from_str("upgrade"))?
                  .to_ascii_lowercase() == "websocket"))
        .unwrap_or(false)
}
