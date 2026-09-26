use crate::i18n::t;
use std::{
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Clip {
    pub path: String,
    pub name: String,
    pub game: String,
    pub size: u64,
    pub modified: u64,
}

pub fn is_video(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| ["mp4", "mkv", "mov"].iter().any(|v| e.eq_ignore_ascii_case(v)))
}

pub fn checked_path(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let clip = path.canonicalize().map_err(|_| t("error.clipNotFound"))?;
    if clip.starts_with(&root) && clip.is_file() && is_video(&clip) {
        Ok(clip)
    } else {
        Err(t("error.clipOutsideOutput"))
    }
}

fn collect(root: &Path, dir: &Path, game: &str, out: &mut Vec<Clip>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if !is_video(&path) || checked_path(root, &path).is_err() {
            continue;
        }
        let Ok(meta) = path.metadata() else { continue };
        out.push(Clip {
            path: path.to_string_lossy().into_owned(),
            name: entry.file_name().to_string_lossy().into_owned(),
            game: game.to_string(),
            size: meta.len(),
            modified: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs()),
        });
    }
}

pub fn list(root: &Path, recording: Option<&Path>) -> Vec<Clip> {
    let mut clips = Vec::new();
    collect(root, root, "", &mut clips);
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                collect(root, &entry.path(), &entry.file_name().to_string_lossy(), &mut clips);
            }
        }
    }
    if let Some(recording) = recording {
        clips.retain(|c| Path::new(&c.path) != recording);
    }
    clips.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.path.cmp(&b.path)));
    clips
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn executable_and_outside_paths_are_rejected_and_canonical_path_returned() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        for path in [root.path().join("x.exe"), outside.path().join("x.mp4"), root.path().join("x.mp4")] {
            std::fs::write(path, b"x").unwrap();
        }
        assert!(checked_path(root.path(), &root.path().join("x.exe")).is_err());
        assert!(checked_path(root.path(), &outside.path().join("x.mp4")).is_err());
        assert_eq!(
            checked_path(root.path(), &root.path().join("./x.mp4")).unwrap(),
            root.path().join("x.mp4").canonicalize().unwrap()
        );
    }
    #[test]
    fn older_clips_remain_accessible_beyond_500() {
        let dir = tempfile::tempdir().unwrap();
        for n in 0..510 {
            std::fs::write(dir.path().join(format!("{n}.mp4")), b"x").unwrap();
        }
        assert_eq!(list(dir.path(), None).len(), 510);
        assert_eq!(list(dir.path(), Some(&dir.path().join("5.mp4"))).len(), 509);
    }
}
