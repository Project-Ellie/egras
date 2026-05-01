//! Opaque base64url-JSON cursor codec shared across domains.
//!
//! Cursors are encoded as `base64url(json(T))` — opaque to the caller.
//! The encoding is NOT stable across versions; clients must treat the
//! `next_cursor` string as an opaque handle and pass it back verbatim.

use base64::Engine as _;
use serde::de::DeserializeOwned;
use serde::Serialize;

#[derive(Debug, Clone, Copy)]
pub struct CursorDecodeError;

pub fn encode<T: Serialize>(value: &T) -> String {
    let json = serde_json::to_vec(value).expect("cursor value must serialize");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json)
}

pub fn decode<T: DeserializeOwned>(raw: &str) -> Result<T, CursorDecodeError> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(raw)
        .map_err(|_| CursorDecodeError)?;
    serde_json::from_slice::<T>(&bytes).map_err(|_| CursorDecodeError)
}
