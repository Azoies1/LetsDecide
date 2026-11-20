//! Gemini Decisions rules that go beyond the shared System One wire format.

use crate::*;
use serde_json::{json, Map, Value};

/// Image MIME types accepted (decisions-api, "Input").
pub(crate) const IMAGE_MIME: [&str; 8] = [
    "image/png",
    "image/jpeg",
    "image/webp",
    "image/heic",
    "image/heif",
    "image/gif",
    "image/bmp",
    "image/tiff",
];

/// Inline data caps: 100 MB per request, 50 MB of it PDF.
const MAX_INLINE: usize = 100 << 20;
const MAX_PDF: usize = 50 << 20;

/// `error.code` values that mean the input was blocked.
const BLOCK_CODES: [&str; 5] = [
    "safety",
    "prohibited_content",
    "blocklist",
    "image_safety",
    "content_blocked",
];

/// Question and option ids: `[A-Za-z0-9_-]{1,128}`.
pub(crate) fn id_ok(s: &str) -> bool {
    (1..=128).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// The `input` field: plain text, or typed blocks when media is attached.
pub(crate) fn input(req: &DecisionRequest) -> Result<Value, Error> {
    let text = match &req.input {
        Value::String(_) => req.input.clone(),
        other => Value::String(other.to_string()),
    };
    if req.media.is_empty() {
        return Ok(text);
    }
    check_sizes(&req.media)?;
    let mut blocks = vec![json!({ "type": "text", "text": text })];
    blocks.extend(req.media.iter().map(block).collect::<Result<Vec<_>, _>>()?);
    Ok(Value::Array(blocks))
}

fn block(m: &Media) -> Result<Value, Error> {
    let unsupported = |msg: String| Error::new(ErrorCode::Unsupported, msg);
    let kind = m
        .kind()
        .ok_or_else(|| unsupported(format!("unsupported media `{}`", m.mime_type)))?;
    if kind == "image" && !IMAGE_MIME.contains(&m.mime_type.as_str()) {
        return Err(unsupported(
            "Gemini does not accept this image MIME type".into(),
        ));
    }
    // Options go through as-is; the API validates them.
    let mut b: Map<String, Value> = m.options.clone().into_iter().collect();
    b.insert("type".into(), kind.into());
    b.insert("mime_type".into(), m.mime_type.clone().into());
    match &m.uri {
        Some(u) => {
            check_uri(u)?;
            b.insert("uri".into(), u.clone().into());
        }
        None => {
            b.insert("data".into(), m.data.clone().into());
        }
    }
    Ok(Value::Object(b))
}

/// HTTPS URLs and Files API URIs only; no `gs://` or YouTube.
fn check_uri(u: &str) -> Result<(), Error> {
    let host = u
        .strip_prefix("https://")
        .map(|r| r.split(['/', '?', '#']).next().unwrap_or(""))
        .ok_or_else(|| Error::new(ErrorCode::Unsupported, "media `uri` must be https://"))?
        .to_ascii_lowercase();
    if host == "youtu.be" || host == "youtube.com" || host.ends_with(".youtube.com") {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "YouTube URLs are not accepted",
        ));
    }
    Ok(())
}

/// Sizes are estimated from the base64 length (padding included).
fn check_sizes(media: &[Media]) -> Result<(), Error> {
    let size = |m: &Media| m.data.len() / 4 * 3;
    let total: usize = media.iter().map(size).sum();
    let pdf: usize = media
        .iter()
        .filter(|m| m.mime_type == "application/pdf")
        .map(size)
        .sum();
    if total > MAX_INLINE || pdf > MAX_PDF {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "inline media is limited to 100 MB per request, 50 MB for PDF",
        ));
    }
    Ok(())
}

/// A 400 whose `error.code` says the input was blocked; returns the message.
pub(crate) fn blocked_by_error(body: &[u8]) -> Option<String> {
    let v = serde_json::from_slice::<Value>(body).ok()?;
    let code = v["error"]["code"].as_str()?;
    BLOCK_CODES
        .contains(&code)
        .then(|| v["error"]["message"].as_str().unwrap_or(code).to_owned())
}
