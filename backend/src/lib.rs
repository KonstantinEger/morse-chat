pub mod request;
pub mod response;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HeaderName(String);

impl HeaderName {
    pub fn from_str(src: &str) -> Self {
        Self(src.to_ascii_lowercase())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

