use std::{pin::Pin, task::{Context, Poll}, sync::Arc, collections::VecDeque};

use frame::{Frame, OpCode};
use pin_project::pin_project;
use futures::Future;
use tokio::{net::TcpStream, task::{self, JoinHandle}, sync::Mutex};
use tokio::sync::mpsc::{self, Sender};

mod frame;

pub struct WebSocket {
    stream_task: JoinHandle<()>,
    recv_queue: Arc<Mutex<VecDeque<Result<Message, MessageError>>>>,
    cmd_channel: Sender<Cmd>,
}

enum Cmd {
    Close,
    Send(Message),
}

enum NextStep {
    Read,
    Write(Cmd),
}

#[derive(Debug, Clone)]
pub enum Message {
    Text(String),
    Binary(Vec<u8>),
}

#[derive(Debug, Clone)]
pub enum MessageError {
    ConnectionClosed,
    InvalidMessage,
    Network,
}

#[pin_project]
struct NextStepFuture<S, C> {
    #[pin]
    stream: S,
    #[pin]
    channel: C,
}

impl WebSocket {
    const CMD_CHANNEL_BUF_SIZE: usize = 10;

    /// Starts a background task reading and writing messages from the stream.
    ///
    /// For sending messages, use [WebSocket::try_send]. For getting a newly
    /// received message from the queue, use [WebSocket::next_message_if_exists].
    /// To close the websocket and with it the `TcpStream`, use [WebSocket::shutdown].
    pub fn new(stream: TcpStream) -> Self {
        let (cmd_channel, mut rx) = mpsc::channel(Self::CMD_CHANNEL_BUF_SIZE);
        let queue = Arc::new(Mutex::new(VecDeque::new()));
        let queue_clone = Arc::clone(&queue);
        let stream_task = task::spawn(async move {
            let mut stream = stream;
            loop {
                let next_step = NextStepFuture::new(stream.peek(&mut [0]), rx.recv()).await;
                match next_step {
                    NextStep::Read => {
                        let msg = read_message_from(&mut stream).await;
                        let should_close = msg.is_err();
                        queue_clone.lock().await.push_back(msg);
                        if should_close {
                            break;
                        }
                    },
                    NextStep::Write(cmd) => {
                        let should_close = if let Cmd::Send(msg) = cmd {
                            let res = write_message_to(msg, &mut stream).await;
                            res.is_err()
                        } else {
                            let _ = close_connection(&mut stream).await;
                            true
                        };
                        if should_close {
                            break;
                        }
                    },
                }
            }
        });
        Self {
            stream_task,
            cmd_channel,
            recv_queue: queue,
        }
    }

    pub async fn shutdown(self) -> Result<(), &'static str> {
        self.cmd_channel
            .send(Cmd::Close)
            .await
            .map_err(|_| "error sending close command to task")?;
        self.stream_task
            .await
            .map_err(|_| "error waiting on task to end")
    }

    /// Returns the next read message if it exists. This function does not wait for a new message.
    pub async fn poll_next_message(&self) -> Option<Result<Message, MessageError>> {
        let mut lock = self.recv_queue.lock().await;
        lock.pop_front()
    }

    pub async fn try_send(&self, msg: Message) -> Result<(), Message> {
        self.cmd_channel
            .send(Cmd::Send(msg))
            .await
            .map_err(|e| e.0.message().unwrap())
    }
}

async fn read_message_from(stream: &mut TcpStream) -> Result<Message, MessageError> {
    let mut message = Vec::new();
    let mut is_text = None;

    loop {
        let mut frame = Frame::try_parse_from(stream)
            .await
            .map_err(|_| MessageError::InvalidMessage)?;

        if is_text.is_none() {
            is_text = Some(matches!(frame.opcode(), OpCode::Text));
        }

        if let Some(mask) = frame.mask() {
            frame::demask(frame.payload_mut(), mask);
        }

        if frame.opcode().is_non_control() {
            message.extend_from_slice(frame.payload());
        }

        if matches!(frame.opcode(), OpCode::Close) {
            Frame::builder()
                .is_final()
                .with_opcode(OpCode::Close)
                .with_payload(frame.payload().to_owned())
                .write_to(stream)
                .await
                .map_err(|_| MessageError::Network)?;
            return Err(MessageError::ConnectionClosed);
        } else if matches!(frame.opcode(), OpCode::Ping) {
            Frame::builder()
                .is_final()
                .with_opcode(OpCode::Pong)
                .with_payload(frame.payload().to_owned())
                .write_to(stream)
                .await
                .map_err(|_| MessageError::Network)?;
        }

        if frame.is_final() {
            break;
        }
    }

    if let Some(true) = is_text {
        Ok(Message::Text(String::from_utf8_lossy(message.as_slice()).to_string()))
    } else {
        Ok(Message::Binary(message))
    }
}

async fn write_message_to(message: Message, stream: &mut TcpStream) -> Result<(), &'static str> {
    let (first_opcode, bytes) = match message {
        Message::Text(text) => (OpCode::Text, text.into_bytes()),
        Message::Binary(bytes) => (OpCode::Binary, bytes),
    };

    if bytes.len() == 0 { return Ok(()); }

    let chunks = bytes.chunks(1024).enumerate().collect::<Vec<_>>();
    let num_chunks = chunks.len();

    for (idx, chunk) in chunks {
        let mut builder = Frame::builder();
        if idx == num_chunks - 1 {
            builder.is_final();
        } else {
            builder.is_not_final();
        }
        if idx == 0 {
            builder.with_opcode(first_opcode);
        } else {
            builder.with_opcode(OpCode::Continuation);
        }
        builder.with_payload(chunk.to_owned())
            .write_to(stream)
            .await?;
    }
    
    Ok(())
}

async fn close_connection(stream: &mut TcpStream) -> Result<(), &'static str> {
    Frame::builder()
        .is_final()
        .with_opcode(OpCode::Close)
        .with_payload(Vec::new())
        .write_to(stream)
        .await
}

impl<S, C> NextStepFuture<S, C> {
    pub fn new(stream: S, channel: C) -> Self {
        Self { stream, channel }
    }
}

impl<S, C> Future for NextStepFuture<S, C>
where
    S: Future<Output = std::io::Result<usize>>,
    C: Future<Output = Option<Cmd>>,
{
    type Output = NextStep;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        match this.stream.poll(ctx) {
            Poll::Ready(_) => return Poll::Ready(NextStep::Read),
            _ => {},
        };
        match this.channel.poll(ctx) {
            Poll::Ready(cmd) => return Poll::Ready(NextStep::Write(cmd.unwrap())),
            _ => return Poll::Pending,
        }
    }
}

impl Cmd {
    pub fn message(self) -> Option<Message> {
        match self {
            Self::Send(m) => Some(m),
            Self::Close => None,
        }
    }
}

#[cfg(test)]
mod tests {
}
