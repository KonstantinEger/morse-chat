use std::collections::HashMap;

use tokio::io::AsyncWriteExt;

use crate::HeaderName;

pub struct Response {
    status: Status,
    headers: HashMap<HeaderName, String>,
    body: Vec<u8>,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    SwitchingProtocols,
    #[default]
    OK,
    BadRequest,
    NotFound,
    InternalServerError,
}

#[derive(Default)]
pub struct Builder {
    status: Status,
    headers: HashMap<HeaderName, String>,
}

impl Response {
    pub fn builder() -> Builder {
        Default::default()
    }

    pub async fn try_write_to<W: AsyncWriteExt + Unpin>(self, mut dest: W) -> anyhow::Result<()> {
        dest.write(&self.into_bytes()).await?;
        Ok(())
    }

    pub fn into_bytes(self) -> Vec<u8> {
        let first_line = format!("HTTP/1.1 {}\r\n", self.status.as_str());
        let headers = self.headers.into_iter()
            .map(|(hn, hv)| format!("{}: {}\r\n", hn.as_str(), hv))
            .collect::<String>();
        
        let complete_header = first_line + &headers + "\r\n";

        let mut result = complete_header.into_bytes();
        result.extend_from_slice(&self.body);
        result
    }
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SwitchingProtocols => "101 Switching Protocols",
            Self::OK => "200 OK",
            Self::BadRequest => "400 Bad Request",
            Self::NotFound => "404 Not Found",
            Self::InternalServerError => "500 Internal Server Error",
        }
    }
}

impl Builder {
    pub fn with_status(&mut self, status: Status) -> &mut Self {
        self.status = status;
        self
    }

    pub fn as_html(&mut self) -> &mut Self {
        self.with_header("content-type", "text/html")
    }

    pub fn with_header<N: AsRef<str>, V: Into<String>>(&mut self, name: N, value: V) -> &mut Self {
        self.headers.insert(HeaderName::from_str(name.as_ref()), value.into());
        self
    }

    pub fn with_body<B: Into<Vec<u8>>>(&mut self, body: B) -> Response {
        Response {
            status: self.status,
            headers: self.headers.clone(),
            body: body.into(),
        }
    }
}
