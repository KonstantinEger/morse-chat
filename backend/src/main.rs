use std::collections::HashMap;
use std::sync::Arc;

use backend::HeaderName;
use tokio::net::{TcpListener, TcpStream};
use sha1::{Sha1, Digest};

use backend::request::{Method, Request};
use backend::response::{Response, Status};
use tokio::sync::Mutex;
use websockets::{WebSocket, Message};

pub struct AppData {
    sockets: HashMap<usize, WebSocket<Box<dyn Fn(Message)>, Box<dyn FnOnce()>, Box<dyn FnOnce()>>>,
}

type SharedAppData = Arc<Mutex<AppData>>;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let server = TcpListener::bind(("127.0.0.1", 8080)).await?;
    let app_data = Arc::new(Mutex::new(AppData { sockets: HashMap::new() }));

    loop {
        let (mut stream, _) = if let Ok(s) = server.accept().await {
            println!("accepted connection");
            s
        } else {
            println!("refused connection");
            continue;
        };
        let request = if let Ok(req) = Request::try_parse_from(&mut stream).await {
            println!("successfully parsed request to {}", req.path());
            println!("request: {:#?}", &req);
            req
        } else {
            println!("failed to parse request");
            let response = Response::builder()
                .with_status(Status::BadRequest)
                .with_body(Vec::new());
            let _ = response.try_write_to(&mut stream).await;
            continue;
        };
        let _ = handle(request, stream, Arc::clone(&app_data)).await;
    }
}

async fn handle(req: Request, mut stream: TcpStream, data: SharedAppData) -> anyhow::Result<()> {
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
    println!("successfully sent response");

    if upgraded_to_ws {
        // save websocket
        tokio::spawn(async move {
            let ws = WebSocket::builder()
                .on_message(Box::new(|m: Message| { dbg!(m); }) as Box<dyn Fn(Message)>)
                .on_close(Box::new(|| { dbg!("error"); }) as Box<dyn FnOnce()>)
                .on_error(Box::new(|| { dbg!("close"); }) as Box<dyn FnOnce()>)
                .build(stream);
            let mut d = data.lock().await;
            let id = d.sockets.len();
            d.sockets.insert(id, ws);
        });
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
