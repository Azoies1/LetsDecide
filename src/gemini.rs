//! Typed content blocks (the `typed_blocks` media encoding, Gemini) and the
//! wider id rule. Accepted MIME types and block codes are dialect data
//! (`spec/dialects/gemini.json`).

use crate::*;
use serde_json::{json, Map, Value};

/// Inline data caps: 100 MB per request, 50 MB of it PDF.
const MAX_INLINE: usize = 100 << 20;
const MAX_PDF: usize = 50 << 20;

/// Question and option ids: `[A-Za-z0-9_-]{1,128}`.
pub(crate) fn id_ok(s: &str) -> bool {
    (1..=128).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Typed blocks: the text first, then one block per media item.
pub(crate) fn blocks(text: Value, media: &[Media]) -> Result<Value, Error> {
    check_sizes(media)?;
    let mut blocks = vec![json!({ "type": "text", "text": text })];
    blocks.extend(media.iter().map(block).collect::<Result<Vec<_>, _>>()?);
    Ok(Value::Array(blocks))
}

fn block(m: &Media) -> Result<Value, Error> {
    let unsupported = |msg: String| Error::new(ErrorCode::Unsupported, msg);
    let kind = m
        .kind()
        .ok_or_else(|| unsupported(format!("unsupported media `{}`", m.mime_type)))?;
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
