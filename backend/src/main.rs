use std::collections::HashMap;
use std::sync::Arc;

use backend::HeaderName;
use tokio::net::{TcpListener, TcpStream};
use sha1::{Sha1, Digest};
use rand::Rng;

use backend::request::{Method, Request};
use backend::response::{Response, Status};
use tokio::sync::Mutex;
use tokio::task;
use websockets::WebSocket;

#[derive(Default)]
struct AppData {
    rooms: HashMap<String, HashMap<usize, WebSocket>>,
}

type SharedAppData = Arc<Mutex<AppData>>;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let server = TcpListener::bind(("127.0.0.1", 8080)).await?;
    let rooms = HashMap::from([(String::from("asdf"), HashMap::new())]);
    let app_data: SharedAppData = Arc::new(Mutex::new(AppData { rooms }));

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
    loop {
        let mut data = app_data.lock().await;
        for (_, room) in &mut data.rooms {
            let mut delete_members = Vec::new();
            // collect messages
            let mut messages = Vec::with_capacity(room.len());
            for (&id, socket) in &*room {
                match socket.poll_next_message().await {
                    Some(Err(e)) => {
                        dbg!(e);
                        delete_members.push(id);
                    },
                    Some(Ok(msg)) => {
                        messages.push((id, msg));
                    },
                    None => {},
                }
            }
            // cleanup
            for id in delete_members {
                room.remove(&id);
            }
            // send messages
            for (sender_id, message) in messages {
                for (_, socket) in (&*room).into_iter().filter(|(&id, _)| id != sender_id) {
                    socket.try_send(message.clone()).await.unwrap();
                }
            }
        }
        drop(data);
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

async fn handle(req: Request, mut stream: TcpStream, app_data: SharedAppData) -> anyhow::Result<()> {
    match (req.method(), req.path()) {
        (Method::Get, "/") | (Method::Get, "/index.html") => {
            // serve index html
            let html = include_str!("../../frontend/index.html");
            Response::builder()
                .as_html()
                .with_body(html)
                .try_write_to(&mut stream)
                .await?;
        },
        (Method::Get, path) if path.starts_with("/ws") => {
            println!("start new ws");
            handle_new_ws(&req, stream, app_data).await;
        },
        (_, path) => {
            Response::builder()
                .with_status(Status::NotFound)
                .with_body(format!("Error 404: no resource with path {} found", path))
                .try_write_to(&mut stream)
                .await?;
        }
    };
    Ok(())
}

async fn handle_new_ws(request: &Request, mut stream: TcpStream, app_data: SharedAppData) {
    let (response, room_name) = if let Some(res) = try_upgrade_to_ws(request) {
        res
    } else {
        let _ = Response::builder()
            .with_status(Status::BadRequest)
            .with_body(Vec::new())
            .try_write_to(&mut stream)
            .await;
        return;
    };
    let mut data = app_data.lock().await;
    let room = if let Some(room) = data.rooms.get_mut(&room_name) {
        room
    } else {
        let _ = Response::builder()
            .with_status(Status::NotFound)
            .with_body(format!("no room with name {} found.", room_name))
            .try_write_to(&mut stream)
            .await;
        return;
    };

    if let Err(e) = response.try_write_to(&mut stream).await {
        dbg!(e);
        return;
    }

    let mut rng = rand::thread_rng();

    let id = rng.gen();
    let socket = WebSocket::new(stream);
    room.insert(id, socket);
}

fn try_upgrade_to_ws(request: &Request) -> Option<(Response, String)> {
    if !fulfills_ws_requirements(request) {
        return None;
    }

    let (_, room) = get_query_params(request.path())
        .find(|(key, _)| *key == "room")?;

    // upgrade to websocket
    let nonce = request.headers().get(&HeaderName::from_str("sec-websocket-key")).unwrap();
    let hash = get_websocket_accept_hash(nonce);
    let resp = Response::builder()
        .with_status(Status::SwitchingProtocols)
        .with_header("connection", "Upgrade")
        .with_header("upgrade", "websocket")
        .with_header("sec-websocket-accept", hash)
        .with_body(Vec::new());
    Some((resp, room.to_owned()))
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
        .map(|prev| prev && req.headers().get(&HeaderName::from_str("sec-websocket-key")).is_some())
        .unwrap_or(false)
}

fn get_query_params(string: &str) -> impl Iterator<Item = (&str, &str)> {
    string.split(&['?', '&'])
        .skip(1)
        .flat_map(|pair| pair.split_once('='))
}
