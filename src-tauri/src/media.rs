//! Video-only protocol. Every request uses the CURRENT root, so old roots lose access immediately.
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};
use tauri::http::{Request, Response};
const CHUNK: u64 = 1024 * 1024;

fn range(value: &str, length: u64) -> Option<(u64, u64)> {
    let (start, end) = value.strip_prefix("bytes=")?.split_once('-')?;
    if length == 0 || end.contains(',') {
        return None;
    }
    let (start, end) = if start.is_empty() {
        let suffix = end.parse::<u64>().ok()?;
        if suffix == 0 {
            return None;
        }
        (length.saturating_sub(suffix), length - 1)
    } else {
        let start = start.parse::<u64>().ok()?;
        (
            start,
            if end.is_empty() {
                length - 1
            } else {
                end.parse::<u64>().ok()?.min(length - 1)
            },
        )
    };
    if start >= length || end < start {
        return None;
    }
    Some((start, end.min(start.saturating_add(CHUNK - 1))))
}

pub fn response(root: &Path, request: Request<Vec<u8>>, allowed: bool) -> Response<Vec<u8>> {
    let error = |status| Response::builder().status(status).body(Vec::new()).unwrap();
    if !allowed || !matches!(request.method().as_str(), "GET" | "HEAD") {
        return error(403);
    }
    let decoded = percent_encoding::percent_decode_str(request.uri().path().trim_start_matches('/')).decode_utf8();
    let Ok(path) = decoded else { return error(400) };
    let Ok(path) = crate::clips::checked_path(root, Path::new(path.as_ref())) else {
        return error(403);
    };
    let Ok(mut file) = File::open(&path) else { return error(404) };
    let Ok(meta) = file.metadata() else { return error(500) };
    let length = meta.len();
    let mime = match path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "mkv" => "video/x-matroska",
        "mov" => "video/quicktime",
        _ => "video/mp4",
    };
    let mut builder = Response::builder()
        .header("Content-Type", mime)
        .header("Accept-Ranges", "bytes")
        .header("Access-Control-Allow-Origin", "*")
        .header("Access-Control-Expose-Headers", "Content-Range")
        .header("Cache-Control", "no-store")
        .header("X-Content-Type-Options", "nosniff");
    if request.method() == "HEAD" {
        return builder.header("Content-Length", length).body(Vec::new()).unwrap();
    }
    let bounds = match request.headers().get("Range") {
        Some(header) => header.to_str().ok().and_then(|value| range(value, length)),
        None if length <= CHUNK => Some((0, length.saturating_sub(1))),
        // Browsers request videos in ranges. Never allocate a multi-GB clip for a plain GET.
        None => None,
    };
    let Some((start, end)) = bounds else {
        return builder
            .status(416)
            .header("Content-Range", format!("bytes */{length}"))
            .body(Vec::new())
            .unwrap();
    };
    let count = if length == 0 { 0 } else { end - start + 1 };
    if request.headers().contains_key("Range") {
        builder = builder.status(206).header("Content-Range", format!("bytes {start}-{end}/{length}"));
    }
    let mut body = Vec::with_capacity(count as usize);
    if file.seek(SeekFrom::Start(start)).is_err() || file.take(count).read_to_end(&mut body).is_err() {
        return error(500);
    }
    builder.header("Content-Length", body.len()).body(body).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ranges_are_bounded_and_invalid_ranges_rejected() {
        assert_eq!(range("bytes=0-", 10 * CHUNK), Some((0, CHUNK - 1)));
        assert_eq!(range("bytes=-20", 100), Some((80, 99)));
        for value in ["bytes=100-", "bytes=9-1", "bytes=-0", "bytes=1-2,3-4", "garbage"] {
            assert!(range(value, 100).is_none());
        }
    }
    #[test]
    fn switching_root_revokes_old_video_and_toast_has_no_access() {
        let old = tempfile::tempdir().unwrap();
        let new = tempfile::tempdir().unwrap();
        let clip = old.path().join("clip.mp4");
        std::fs::write(&clip, b"test video").unwrap();
        let uri = format!(
            "clipcat://localhost/{}",
            percent_encoding::utf8_percent_encode(&clip.to_string_lossy(), percent_encoding::NON_ALPHANUMERIC)
        );
        let req = || Request::builder().uri(&uri).header("Range", "bytes=0-3").body(Vec::new()).unwrap();
        let result = response(old.path(), req(), true);
        assert_eq!(result.status(), 206);
        assert_eq!(result.body(), b"test");
        assert_eq!(response(new.path(), req(), true).status(), 403);
        assert_eq!(response(old.path(), req(), false).status(), 403);
    }
}
