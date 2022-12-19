use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    is_final: bool,
    opcode: OpCode,
    mask: Option<[u8; 4]>,
    payload: Vec<u8>,
}

impl Frame {
    pub fn builder() -> Builder {
        Default::default()
    }

    pub fn is_final(&self) -> bool {
        self.is_final
    }

    pub fn opcode(&self) -> OpCode {
        self.opcode
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    pub fn payload_mut(&mut self) -> &mut [u8] {
        &mut self.payload
    }

    pub fn mask(&self) -> Option<[u8; 4]> {
        self.mask
    }

    pub async fn try_parse_from<R: AsyncReadExt + Unpin>(reader: &mut R) -> Result<Self, &'static str> {
        let mut first_two = [0; 2];
        reader.read(&mut first_two).await.map_err(|_| "error reading first two header bytes")?;
        
        let is_final = first_two[0] >> 7 != 0;
        let opcode = OpCode::try_from(first_two[0] & 0x0f)?;
        let is_masked = first_two[1] >> 7 != 0;
        let payload_len = match first_two[1] & 0x7f {
            126 => {
                let mut next = [0; 2];
                reader.read(&mut next).await.map_err(|_| "error reading payload length")?;
                u16::from_be_bytes(next) as u64
            },
            127 => {
                let mut next = [0; 8];
                reader.read(&mut next).await.map_err(|_| "error reading payload length")?;
                u64::from_be_bytes(next)
            },
            len => len as u64,
        };

        let mask = if is_masked {
            let mut next = [0; 4];
            reader.read(&mut next).await.map_err(|_| "error reading masking key")?;
            Some(next)
        } else {
            None
        };

        let mut payload = vec![0; payload_len as usize];
        reader.read(&mut payload[..]).await.map_err(|_| "error reading payload")?;

        let frame = Frame { is_final, opcode, mask, payload };
        Ok(frame)
    }

    pub async fn write_to<W: AsyncWriteExt + Unpin>(self, dest: &mut W) -> Result<(), &'static str> {
        let opcode: u8 = self.opcode.into();
        let is_final = if self.is_final { 0x80 } else { 0x0 };
        let first = is_final | opcode;

        dest.write(&[first]).await.map_err(|_| "error writing first byte")?;
        
        let is_masked = if self.mask.is_some() { 0x80 } else { 0x0 };
        let actual_len = self.payload.len();
        let write_len_result = if actual_len < 126 {
            let bytes = [actual_len as u8 | is_masked];
            dest.write(&bytes).await
        } else if 126 <= actual_len && actual_len <= 0x7fff {
            let [a, b] = (actual_len as u16).to_be_bytes();
            dest.write(&[126 | is_masked, a, b]).await
        } else {
            let [a, b, c, d, e, f, g, h] = (actual_len as u64).to_be_bytes();
            dest.write(&[127 | is_masked, a, b, c, d, e, f, g, h]).await
        };

        write_len_result.map_err(|_| "error writing payload length")?;

        if let Some(mask) = self.mask {
            dest.write(&mask).await.map_err(|_| "error writing mask")?;
        }

        dest.write(&self.payload.as_slice())
            .await
            .map_err(|_| "error writing payload")?;

        Ok(())
    }
}

pub fn demask(data: &mut [u8], mask: [u8; 4]) {
    data.into_iter()
        .zip(mask.into_iter().cycle())
        .for_each(|(dr, m)| {
            *dr = *dr ^ m
        });
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpCode {
    Continuation,
    Text,
    Binary,
    NonControlReserved(u8),
    Close,
    Ping,
    Pong,
    ControlReserved(u8),
}

impl OpCode {
    pub fn is_control(&self) -> bool {
        matches!(self, Self::Close | Self::Ping | Self::Pong | Self::ControlReserved(_))
    }

    pub fn is_non_control(&self) -> bool {
        !self.is_control()
    }
}

impl TryFrom<u8> for OpCode {
    type Error = &'static str;

    fn try_from(n: u8) -> Result<Self, Self::Error> {
        match n {
            0x0 => Ok(Self::Continuation),
            0x1 => Ok(Self::Text),
            0x2 => Ok(Self::Binary),
            0x8 => Ok(Self::Close),
            0x9 => Ok(Self::Ping),
            0xa => Ok(Self::Pong),
            other if 3 <= other && other <= 7 => Ok(Self::NonControlReserved(other)),
            other if 0xb <= other && other <= 0xf => Ok(Self::ControlReserved(other)),
            _ => Err("unrecognized opcode"),
        }
    }
}

impl Into<u8> for OpCode {
    fn into(self) -> u8 {
        match self {
            Self::Continuation => 0x0,
            Self::Text => 0x1,
            Self::Binary => 0x2,
            Self::NonControlReserved(c) => c,
            Self::Close => 0x8,
            Self::Ping => 0x9,
            Self::Pong => 0xa,
            Self::ControlReserved(c) => c,
        }
    }
}

pub struct Builder {
    is_final: bool,
    opcode: OpCode,
    mask: Option<[u8; 4]>,
}

impl Builder {
    pub fn is_final(&mut self) -> &mut Self {
        self.is_final = true;
        self
    }

    pub fn is_not_final(&mut self) -> &mut Self {
        self.is_final = false;
        self
    }

    pub fn with_opcode(&mut self, code: OpCode) -> &mut Self {
        self.opcode = code;
        self
    }

    pub fn with_mask(&mut self, mask: [u8; 4]) -> &mut Self {
        self.mask = Some(mask);
        self
    }

    pub fn with_payload(&mut self, payload: Vec<u8>) -> Frame {
        Frame {
            is_final: self.is_final,
            opcode: self.opcode,
            mask: self.mask,
            payload,
        }
    }
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            is_final: true,
            opcode: OpCode::Text,
            mask: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use super::*;

    #[tokio::test]
    async fn test_parse_unmasked_text() -> Result<(), Box<dyn Error>> {
        let data = [0x81, 0x05, 0x48, 0x65, 0x6c, 0x6c, 0x6f];
        let frame = Frame::try_parse_from(&mut &data[..]).await?;
        assert!(frame.is_final());
        assert_eq!(frame.opcode, OpCode::Text);
        assert!(frame.mask().is_none());
        assert_eq!(frame.payload(), "Hello".as_bytes());
        Ok(())
    }

    #[test]
    fn test_demask() {
        let mask = [0x37, 0xfa, 0x21, 0x3d];
        let mut data = [0x7f, 0x9f, 0x4d, 0x51, 0x58];
        demask(&mut data, mask);
        assert_eq!(&data[..], "Hello".as_bytes());
    }

    #[tokio::test]
    async fn test_parse_masked_text() -> Result<(), Box<dyn Error>> {
        let data = [0x81, 0x85, 0x37, 0xfa, 0x21, 0x3d, 0x7f, 0x9f, 0x4d, 0x51, 0x58];
        let frame = Frame::try_parse_from(&mut &data[..]).await?;
        assert!(frame.is_final());
        assert_eq!(frame.opcode(), OpCode::Text);
        assert_eq!(frame.mask(), Some([0x37, 0xfa, 0x21, 0x3d]));
        assert_eq!(frame.payload(), &[0x7f, 0x9f, 0x4d, 0x51, 0x58]);
        Ok(())
    }

    #[tokio::test]
    async fn test_parse_non_final() -> Result<(), Box<dyn Error>> {
        let data = [0x01, 0x05, 0x48, 0x65, 0x6c, 0x6c, 0x6f];
        let frame = Frame::try_parse_from(&mut &data[..]).await?;
        assert!(!frame.is_final());
        Ok(())
    }

    #[tokio::test]
    async fn test_write_unmasked() -> Result<(), Box<dyn Error>> {
        let data = [0x81, 0x05, 0x48, 0x65, 0x6c, 0x6c, 0x6f];
        let frame = Frame::builder()
            .is_final()
            .with_opcode(OpCode::Text)
            .with_payload(vec![0x48, 0x65, 0x6c, 0x6c, 0x6f]);
        let mut buffer = Vec::with_capacity(data.len());
        frame.write_to(&mut buffer).await?;
        assert_eq!(&buffer, &data);
        Ok(())
    }

    #[tokio::test]
    async fn test_write_masked() -> Result<(), Box<dyn Error>> {
        let data = [0x81, 0x85, 0x37, 0xfa, 0x21, 0x3d, 0x7f, 0x9f, 0x4d, 0x51, 0x58];
        let frame = Frame::builder()
            .is_final()
            .with_opcode(OpCode::Text)
            .with_mask([0x37, 0xfa, 0x21, 0x3d])
            .with_payload(vec![0x7f, 0x9f, 0x4d, 0x51, 0x58]);
        let mut buffer = Vec::with_capacity(data.len());
        frame.write_to(&mut buffer).await?;
        assert_eq!(&buffer, &data);
        Ok(())
    }
}

