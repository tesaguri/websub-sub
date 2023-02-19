use base64::display::Base64Display;
use base64::Engine;

pub fn encode(id: &[u8; 8]) -> Base64Display<'_, 'static, base64::engine::GeneralPurpose> {
    // Strip the most significant zeroes.
    let end = memchr::memrchr(b'\0', &id[..]).unwrap_or(id.len());
    let id = &id[..end];
    Base64Display::new(id, &base64::engine::general_purpose::URL_SAFE_NO_PAD)
}

pub fn decode(id: &str) -> Option<u64> {
    if id.len() > 11 {
        return None;
    }
    let mut buf = [0; 9];
    let n = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode_slice(id, &mut buf)
        .ok()?;
    if n > 8 {
        return None;
    }
    Some(u64::from_le_bytes(buf[..8].try_into().unwrap()))
}
