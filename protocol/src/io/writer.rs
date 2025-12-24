/// Helper for building binary payloads
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    pub fn write_u8(&mut self, value: u8) {
        self.buf.push(value);
    }

    pub fn write_u16(&mut self, value: u16) {
        self.buf.extend_from_slice(&value.to_be_bytes());
    }

    pub fn write_u32(&mut self, value: u32) {
        self.buf.extend_from_slice(&value.to_be_bytes());
    }

    pub fn write_u64(&mut self, value: u64) {
        self.buf.extend_from_slice(&value.to_be_bytes());
    }

    pub fn write_bool(&mut self, value: bool) {
        self.buf.push(if value { 1 } else { 0 });
    }

    pub fn write_string(&mut self, s: &str) {
        self.write_u16(s.len() as u16);
        self.buf.extend_from_slice(s.as_bytes());
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    pub fn position(&self) -> usize {
        self.buf.len()
    }

    pub fn reserve_u16(&mut self) -> usize {
        let pos = self.buf.len();
        self.buf.extend_from_slice(&[0u8; 2]);
        pos
    }

    pub fn write_u16_at(&mut self, pos: usize, value: u16) {
        let bytes = value.to_be_bytes();
        self.buf[pos] = bytes[0];
        self.buf[pos + 1] = bytes[1];
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.buf
    }
}
