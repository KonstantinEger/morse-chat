use std::collections::HashMap;
use std::sync::Arc;

use backend::HeaderName;
use rand::Rng;
use sha1::{Digest, Sha1};
use tokio::net::{TcpListener, TcpStream};

use backend::request::{Method, Request};
use backend::response::{Response, Status};
use tokio::sync::Mutex;
use tokio::task;
use tracing::{debug, info, trace, warn};
use websockets::WebSocket;

const MAX_ROOM_NUMBER: usize = 20;

#[derive(Default)]
struct AppData {
    rooms: HashMap<String, RoomData>,
}

struct RoomData {
    pub sockets: HashMap<usize, WebSocket>,
    pub is_deletable: bool,
}

type SharedAppData = Arc<Mutex<AppData>>;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .init();
    info!("starting server.");
    let (ip, port) = ("0.0.0.0", 8080);
    let server = TcpListener::bind((ip, port)).await?;
    info!(ip, port, "bound tcp server.");
    let rooms = HashMap::from([(String::from("roomForAll"), RoomData::new())]);
    let app_data: SharedAppData = Arc::new(Mutex::new(AppData { rooms }));

    let _listener_task = task::spawn(msg_listener_task(Arc::clone(&app_data)));

    loop {
        let (mut stream, _) = if let Ok(stream) = server.accept().await {
            info!(
                addr = stream.1.to_string(),
                "successfully accepted new tcp stream."
            );
            stream
        } else {
            debug!("failed to accept tcp stream.");
            continue;
        };
        let request = if let Ok(req) = Request::try_parse_from(&mut stream).await {
            info!(
                method = req.method().to_string(),
                path = req.path(),
                "successfully parsed request."
            );
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

#[tracing::instrument(skip(app_data))]
async fn msg_listener_task(app_data: SharedAppData) {
    loop {
        let mut data = app_data.lock().await;
        let mut delete_rooms = Vec::new();
        for (room_name, room) in &mut data.rooms {
            let mut delete_members = Vec::new();
            // collect messages
            let mut messages = Vec::with_capacity(room.sockets.len());
            for (&id, socket) in &room.sockets {
                match socket.poll_next_message().await {
                    Some(Err(e)) => {
                        debug!(error = ?e, id, "error while polling next message.");
                        delete_members.push(id);
                    }
                    Some(Ok(msg)) => {
                        trace!(?msg, id, room_name);
                        messages.push((id, msg));
                    }
                    None => {}
                }
            }
            // cleanup
            for id in delete_members {
                debug!(id, room_name, "removing member from room.");
                room.sockets.remove(&id);
            }
            if room.sockets.len() == 0 && room.is_deletable {
                delete_rooms.push(room_name.clone());
            }
            // send messages
            for (sender_id, message) in messages {
                for (peer_id, socket) in room.sockets.iter().filter(|(&id, _)| id != sender_id) {
                    trace!(sender_id, peer_id, "sending message to other room member.");
                    let r = socket.try_send(message.clone()).await;
                    if let Err(error) = r {
                        debug!(?error, sender_id, peer_id, "error sending message.");
                    }
                }
            }
        }
        for room_name in delete_rooms {
            info!(room_name, "removing room");
            data.rooms.remove(&room_name);
        }
        drop(data);
        // 120 Hz
        tokio::time::sleep(std::time::Duration::from_millis(8)).await;
    }
}

#[tracing::instrument(skip(req, stream, app_data), fields(http.ip = ?stream.peer_addr()))]
async fn handle(
    req: Request,
    mut stream: TcpStream,
    app_data: SharedAppData,
) -> anyhow::Result<()> {
    match (req.method(), req.path()) {
        (Method::Get, path) if path.starts_with("/chat") => {
            let html = include_str!("../../frontend/chat.html");
            Response::builder()
                .as_html()
                .with_body(html)
                .try_write_to(&mut stream)
                .await?;
            info!("successfully sent response");
        }
        (Method::Get, "/scripts/chat.js") => {
            Response::builder()
                .as_js()
                .with_body(include_str!("../../frontend/scripts/chat.js"))
                .try_write_to(&mut stream)
                .await?;
            info!("successfully sent response");
        }
        (Method::Get, "/scripts/index.js") => {
            Response::builder()
                .as_js()
                .with_body(include_str!("../../frontend/scripts/index.js"))
                .try_write_to(&mut stream)
                .await?;
            info!("successfully sent response");
        }
        (Method::Get, "/styles/style.css") => {
            Response::builder()
                .as_css()
                .with_body(include_str!("../../frontend/styles/style.css"))
                .try_write_to(&mut stream)
                .await?;
            info!("successfully sent response");
        }
        (Method::Get, path) if path.starts_with("/ws") => {
            handle_new_ws(&req, stream, app_data).await;
        }
        (Method::Get, "/") | (Method::Get, "/index.html") => {
            // serve index html
            let html = include_str!("../../frontend/index.html");
            Response::builder()
                .as_html()
                .with_body(html)
                .try_write_to(&mut stream)
                .await?;
            info!("successfully sent response");
        }
        (Method::Get, "/api/rooms") => {
            let names = app_data
                .lock()
                .await
                .rooms
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            Response::builder()
                .as_json()
                .with_body(format!("{:?}", names))
                .try_write_to(&mut stream)
                .await?;
            info!("successfully sent response");
        }
        (Method::Get, "/api/gen-room") => {
            info!("room creation requested");
            let resp = handle_new_room(app_data).await;
            resp.try_write_to(&mut stream).await?;
            info!("successfully sent response ");
        }
        (_, path) => {
            Response::builder()
                .with_status(Status::NotFound)
                .with_body(format!("Error 404: no resource with path {} found", path))
                .try_write_to(&mut stream)
                .await?;
            info!("successfully sent response");
        }
    };
    Ok(())
}

#[tracing::instrument(skip(app_data))]
async fn handle_new_room(app_data: SharedAppData) -> Response {
    let rng = rand::thread_rng();
    let name: String = rng
        .sample_iter(rand::distributions::Alphanumeric)
        .take(6)
        .map(char::from)
        .collect();
    let mut data = app_data.lock().await;
    if data.rooms.len() >= MAX_ROOM_NUMBER {
        warn!("maximum number of rooms reached. creation denied.");
        Response::builder()
            .with_status(Status::Forbidden)
            .as_json()
            .with_body("{ \"status\": 1, \"message\": \"Rooms at capacity.\"}")
    } else {
        data.rooms.insert(name.clone(), RoomData::new());
        info!(name, "room created.");
        Response::builder()
            .with_status(Status::OK)
            .as_json()
            .with_body(format!("{{ \"status\": 0, \"name\": {:?}}}", name))
    }
}

#[tracing::instrument(skip(app_data, request, stream))]
async fn handle_new_ws(request: &Request, mut stream: TcpStream, app_data: SharedAppData) {
    let (response, room_name) = if let Some(res) = try_upgrade_to_ws(request) {
        info!("successfully upgraded to websocket.");
        res
    } else {
        info!("failed to upgrade to websocket.");
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
        info!("tried to join non-existent room. answering with 404.");
        let _ = Response::builder()
            .with_status(Status::NotFound)
            .with_body(format!("no room with name {} found.", room_name))
            .try_write_to(&mut stream)
            .await;
        return;
    };

    if let Err(e) = response.try_write_to(&mut stream).await {
        debug!(?e, "error writing response to stream.");
        return;
    }

    let mut rng = rand::thread_rng();

    let id = rng.gen();
    let socket = WebSocket::new(stream);
    room.sockets.insert(id, socket);
    room.is_deletable = true;
}

#[tracing::instrument]
fn try_upgrade_to_ws(request: &Request) -> Option<(Response, String)> {
    if !fulfills_ws_requirements(request) {
        debug!("request does not fulfill ws requirements.");
        return None;
    }

    let (_, room) = get_query_params(request.path()).find(|(key, _)| *key == "room")?;

    // upgrade to websocket
    let nonce = request
        .headers()
        .get(&HeaderName::from_str("sec-websocket-key"))?;
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
        .and_then(|has_conn| {
            Some(
                has_conn
                    && req
                        .headers()
                        .get(&HeaderName::from_str("upgrade"))?
                        .to_ascii_lowercase()
                        == "websocket",
            )
        })
        .map(|prev| {
            prev && req
                .headers()
                .get(&HeaderName::from_str("sec-websocket-key"))
                .is_some()
        })
        .unwrap_or(false)
}

fn get_query_params(string: &str) -> impl Iterator<Item = (&str, &str)> {
    string
        .split(&['?', '&'])
        .skip(1)
        .flat_map(|pair| pair.split_once('='))
}

impl RoomData {
    pub fn new() -> Self {
        Self {
            sockets: HashMap::new(),
            is_deletable: false,
        }
    }
}
