use std::{pin::Pin, task::{Context, Poll}, sync::Arc, collections::VecDeque};

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
    pub async fn next_message_if_exists(&self) -> Option<Result<Message, MessageError>> {
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
    todo!()
}

async fn write_message_to(message: Message, stream: &mut TcpStream) -> Result<(), &'static str> {
    todo!()
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
