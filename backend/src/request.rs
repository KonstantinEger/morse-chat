use std::collections::HashMap;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

use crate::HeaderName;

#[derive(Debug)]
pub struct Request {
    method: Method,
    path: String,
    version: String,
    headers: HashMap<HeaderName, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Delete,
}

impl Request {
    pub async fn try_parse_from<R: AsyncReadExt + Unpin>(src: R) -> anyhow::Result<Self> {
        let mut r = BufReader::new(src);

        let mut first_line = String::new();
        r.read_line(&mut first_line).await?;
        let mut first_line_split = first_line.split(' ');
        let method = first_line_split
            .next()
            .ok_or(ParseError("expected HTTP method"))?;
        let path = first_line_split
            .next()
            .ok_or(ParseError("expected path"))?
            .to_owned();
        let version = first_line_split
            .next()
            .ok_or(ParseError("expected HTTP version"))?
            .trim()
            .to_owned();

        let method = match method.to_ascii_uppercase().as_str() {
            "GET" => Method::Get,
            "POST" => Method::Post,
            "PUT" => Method::Put,
            "DELETE" => Method::Delete,
            _ => return Err(ParseError("expected HTTP method").into()),
        };

        let mut headers = HashMap::new();
        loop {
            let mut line = String::new();
            r.read_line(&mut line).await?;
            if line.trim().is_empty() {
                break;
            }

            let (name, value) = line
                .split_once(':')
                .ok_or(ParseError("expected HTTP header"))?;
            headers.insert(HeaderName::from_str(name), value.trim().to_owned());
        }

        let req = Self {
            method,
            path,
            version,
            headers,
        };
        Ok(req)
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn method(&self) -> Method {
        self.method
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn headers(&self) -> &HashMap<HeaderName, String> {
        &self.headers
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ParseError(&'static str);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for ParseError {}

impl std::fmt::Display for Method {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        match self {
            Self::Get => write!(f, "GET"),
            Self::Post => write!(f, "POST"),
            Self::Put => write!(f, "PUT"),
            Self::Delete => write!(f, "DELETE"),
        }
    }
}
