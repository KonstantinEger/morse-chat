use tokio::net::TcpStream;
use std::marker::PhantomData;
use frame::{Frame, OpCode};

mod frame;

pub struct WebSocket<M, C, E> {
    stream: TcpStream,
    on_message: M,
    on_close: C,
    on_error: E,
}

impl<M, C, E> WebSocket<M, C, E> {
    pub fn builder() -> Builder<M, C, E, Unset, Unset, Unset> {
        Builder::new()
    }

    async fn next_message(&mut self) -> Result<Message, MessageError> {
        let mut message = Vec::new();
        let mut is_text = None;

        loop {
            let mut frame = Frame::try_parse_from(&mut self.stream)
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
                    .write_to(&mut self.stream)
                    .await
                    .map_err(|_| MessageError::Network)?;
                return Err(MessageError::ConnectionClosed);
            } else if matches!(frame.opcode(), OpCode::Ping) {
                Frame::builder()
                    .is_final()
                    .with_opcode(OpCode::Pong)
                    .with_payload(frame.payload().to_owned())
                    .write_to(&mut self.stream)
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
}

impl<M, C, E> WebSocket<M, C, E>
where
    M: Fn(Message),
    C: FnOnce(),
    E: FnOnce(),
{
    pub async fn listen(mut self) {
        loop {
            match self.next_message().await {
                Ok(msg) => {
                    let on_msg = &self.on_message;
                    on_msg(msg);
                },
                Err(MessageError::ConnectionClosed) => {
                    let on_close = self.on_close;
                    on_close();
                    break;
                },
                Err(_) => {
                    let on_err = self.on_error;
                    on_err();
                    break;
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Text(String),
    Binary(Vec<u8>),
}

pub enum MessageError {
    ConnectionClosed,
    InvalidMessage,
    Network,
}

pub struct Set();
pub struct Unset();

pub struct Builder<M, C, E, MF, CF, EF> {
    on_close: Option<C>,
    on_message: Option<M>,
    on_error: Option<E>,
    _p: PhantomData<(MF, CF, EF)>,
}

impl<M, C, E> Builder<M, C, E, Unset, Unset, Unset> {
    pub fn new() -> Self {
        Self { on_close: None, on_message: None, on_error: None, _p: PhantomData }
    }
}

impl<M, C, E, CF, EF> Builder<M, C, E, Unset, CF, EF> {
    pub fn on_message(self, on_message: M) -> Builder<M, C, E, Set, CF, EF> {
        Builder {
            on_message: Some(on_message),
            on_error: self.on_error,
            on_close: self.on_close,
            _p: PhantomData,
        }
    }
}

impl<M, C, E, MF, EF> Builder<M, C, E, MF, Unset, EF> {
    pub fn on_close(self, on_close: C) -> Builder<M, C, E, MF, Set, EF> {
        Builder {
            on_message: self.on_message,
            on_error: self.on_error,
            on_close: Some(on_close),
            _p: PhantomData,
        }
    }
}

impl<M, C, E, MF, CF> Builder<M, C, E, MF, CF, Unset> {
    pub fn on_error(self, on_error: E) -> Builder<M, C, E, MF, CF, Set> {
        Builder {
            on_message: self.on_message,
            on_error: Some(on_error),
            on_close: self.on_close,
            _p: PhantomData,
        }
    }
}

impl<M, C, E> Builder<M, C, E, Set, Set, Set> {
    pub fn build(self, stream: TcpStream) -> WebSocket<M, C, E> {
        WebSocket {
            stream,
            on_message: self.on_message.unwrap(),
            on_close: self.on_close.unwrap(),
            on_error: self.on_error.unwrap(),
        }
    }
}

#[cfg(test)]
mod tests {
}
