use base64::display::Base64Display;

pub fn encode(id: &[u8; 8]) -> Base64Display<'_> {
    // Strip the most significant zeroes.
    let end = memchr::memrchr(b'\0', &id[..]).unwrap_or(id.len());
    Base64Display::with_config(&id[..end], base64::URL_SAFE_NO_PAD)
}

pub fn decode(id: &str) -> Option<u64> {
    if id.len() > 11 {
        return None;
    }
    let mut buf = [0; 9];
    let n = base64::decode_config_slice(id, base64::URL_SAFE_NO_PAD, &mut buf).ok()?;
    if n > 8 {
        return None;
    }
    Some(u64::from_le_bytes(buf[..8].try_into().unwrap()))
}
