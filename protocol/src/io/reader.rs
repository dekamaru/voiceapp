use crate::error::ProtocolError;

/// Helper for reading binary data with automatic cursor advancement
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn read_u8(&mut self) -> Result<u8, ProtocolError> {
        if self.pos >= self.data.len() {
            return Err(ProtocolError::PacketTooShort {
                expected: self.pos + 1,
                got: self.data.len(),
            });
        }
        let value = self.data[self.pos];
        self.pos += 1;
        Ok(value)
    }

    pub fn read_u16(&mut self) -> Result<u16, ProtocolError> {
        if self.pos + 2 > self.data.len() {
            return Err(ProtocolError::PacketTooShort {
                expected: self.pos + 2,
                got: self.data.len(),
            });
        }
        let bytes: [u8; 2] = self.data[self.pos..self.pos + 2].try_into().unwrap();
        self.pos += 2;
        Ok(u16::from_be_bytes(bytes))
    }

    pub fn read_u32(&mut self) -> Result<u32, ProtocolError> {
        if self.pos + 4 > self.data.len() {
            return Err(ProtocolError::PacketTooShort {
                expected: self.pos + 4,
                got: self.data.len(),
            });
        }
        let bytes: [u8; 4] = self.data[self.pos..self.pos + 4].try_into().unwrap();
        self.pos += 4;
        Ok(u32::from_be_bytes(bytes))
    }

    pub fn read_u64(&mut self) -> Result<u64, ProtocolError> {
        if self.pos + 8 > self.data.len() {
            return Err(ProtocolError::PacketTooShort {
                expected: self.pos + 8,
                got: self.data.len(),
            });
        }
        let bytes: [u8; 8] = self.data[self.pos..self.pos + 8].try_into().unwrap();
        self.pos += 8;
        Ok(u64::from_be_bytes(bytes))
    }

    pub fn read_bool(&mut self) -> Result<bool, ProtocolError> {
        Ok(self.read_u8()? != 0)
    }

    pub fn read_string(&mut self) -> Result<String, ProtocolError> {
        let len = self.read_u16()? as usize;
        if self.pos + len > self.data.len() {
            return Err(ProtocolError::PacketTooShort {
                expected: self.pos + len,
                got: self.data.len(),
            });
        }
        let bytes = &self.data[self.pos..self.pos + len];
        self.pos += len;
        String::from_utf8(bytes.to_vec()).map_err(|_| ProtocolError::InvalidUtf8)
    }

    pub fn remaining(&self) -> &[u8] {
        &self.data[self.pos..]
    }
}
