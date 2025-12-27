/// Helper for building binary payloads.
#[derive(Default)]
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn write_u8(&mut self, value: u8) {
        self.buf.push(value);
    }

    #[inline]
    pub fn write_u16(&mut self, value: u16) {
        self.buf.extend_from_slice(&value.to_be_bytes());
    }

    #[inline]
    pub fn write_u32(&mut self, value: u32) {
        self.buf.extend_from_slice(&value.to_be_bytes());
    }

    #[inline]
    pub fn write_u64(&mut self, value: u64) {
        self.buf.extend_from_slice(&value.to_be_bytes());
    }

    #[inline]
    pub fn write_bool(&mut self, value: bool) {
        self.buf.push(u8::from(value));
    }

    /// Writes a length-prefixed string. Panics if string exceeds 65535 bytes.
    #[inline]
    pub fn write_string(&mut self, s: &str) {
        self.write_u16(s.len().try_into().expect("string too long"));
        self.buf.extend_from_slice(s.as_bytes());
    }

    #[inline]
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    #[inline]
    #[must_use]
    pub fn position(&self) -> usize {
        self.buf.len()
    }

    #[inline]
    pub fn reserve_u16(&mut self) -> usize {
        let pos = self.buf.len();
        self.buf.extend_from_slice(&[0u8; 2]);
        pos
    }

    #[inline]
    pub fn write_u16_at(&mut self, pos: usize, value: u16) {
        self.buf[pos..pos + 2].copy_from_slice(&value.to_be_bytes());
    }

    #[inline]
    #[must_use]
    pub fn into_vec(self) -> Vec<u8> {
        self.buf
    }
}
