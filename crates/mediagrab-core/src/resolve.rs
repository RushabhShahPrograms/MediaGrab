//! Step 1 of a batch: turn a pasted link into a list of items, WITHOUT downloading media.
//!
//! `--flat-playlist` makes yt-dlp list a playlist's entries quickly (title + url)
//! instead of fully extracting every video. A plain single-video URL still works:
//! yt-dlp just prints that one video.

use crate::parse::parse_entries;
use crate::types::Item;
use std::process::Stdio;
use tokio::process::Command;

pub async fn resolve(
    ytdlp: &str,
    extra_args: &[String],
    url: &str,
    first_id: usize,
) -> Result<Vec<Item>, String> {
    let out = Command::new(ytdlp)
        .args([
            "--flat-playlist",
            "--dump-json",
            "--no-warnings",
            "--ignore-errors", // one private/deleted entry must not kill the whole playlist
        ])
        .args(extra_args)
        .arg(url)
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| spawn_error(ytdlp, e))?;

    let stdout = String::from_utf8_lossy(&out.stdout);
    let items = parse_entries(&stdout, first_id);
    if items.is_empty() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let msg = stderr
            .lines()
            .rev()
            .find(|l| l.contains("ERROR"))
            .or_else(|| stderr.lines().rev().find(|l| !l.trim().is_empty()))
            .unwrap_or("no downloadable items found")
            .trim()
            .to_string();
        return Err(msg);
    }
    Ok(items)
}

pub(crate) fn spawn_error(ytdlp: &str, e: std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::NotFound {
        format!(
            "could not start '{ytdlp}': not found. Install yt-dlp (and ffmpeg) and make sure it is on your PATH."
        )
    } else {
        format!("could not start '{ytdlp}': {e}")
    }
}
