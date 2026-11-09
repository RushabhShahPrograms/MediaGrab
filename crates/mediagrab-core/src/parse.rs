//! Pure parsing helpers: no processes, no I/O. Everything here is unit tested.

use crate::types::Item;
use std::path::PathBuf;

/// Markers we ask yt-dlp to put in front of the lines we care about
/// (see `download::build_args`). They make parsing unambiguous.
pub const PROGRESS_MARK: &str = "[[P]]";
pub const POST_MARK: &str = "[[PP]]";
pub const FILE_MARK: &str = "[[F]]";

#[derive(Debug, PartialEq)]
pub enum Line {
    Progress {
        percent: Option<f32>,
        speed: Option<String>,
        eta: Option<String>,
    },
    /// A post-processor (ffmpeg convert / merge / tag) started or finished.
    PostProcess,
    /// Final path of the downloaded file.
    File(PathBuf),
    Error(String),
    Other,
}

/// Parse one line of yt-dlp output (stdout or stderr).
pub fn parse_line(line: &str) -> Line {
    let line = line.trim();
    if let Some(rest) = line.strip_prefix(PROGRESS_MARK) {
        // downloaded | total | total_estimate | speed | eta   ("NA"/"None" when unknown)
        let f: Vec<&str> = rest.split('|').collect();
        let num = |i: usize| f.get(i).and_then(|s| s.trim().parse::<f64>().ok());
        let downloaded = num(0);
        let total = num(1).filter(|t| *t > 0.0).or(num(2).filter(|t| *t > 0.0));
        let percent = match (downloaded, total) {
            (Some(d), Some(t)) => Some(((d / t) * 100.0).clamp(0.0, 100.0) as f32),
            _ => None,
        };
        return Line::Progress {
            percent,
            speed: num(3).map(human_speed),
            eta: num(4).map(human_eta),
        };
    }
    if line.starts_with(POST_MARK) {
        return Line::PostProcess;
    }
    if let Some(path) = line.strip_prefix(FILE_MARK) {
        if !path.is_empty() {
            return Line::File(PathBuf::from(path));
        }
    }
    if line.starts_with("ERROR:") {
        return Line::Error(line.trim_start_matches("ERROR:").trim().to_string());
    }
    Line::Other
}

pub fn human_speed(bytes_per_sec: f64) -> String {
    const UNITS: [&str; 4] = ["B/s", "KiB/s", "MiB/s", "GiB/s"];
    let mut v = bytes_per_sec;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    format!("{:.1} {}", v, UNITS[i])
}

pub fn human_eta(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

/// Parse the output of `yt-dlp --flat-playlist --dump-json`: one JSON object per line.
/// `first_id` is the id given to the first item; ids then count up.
pub fn parse_entries(stdout: &str, first_id: usize) -> Vec<Item> {
    let mut items: Vec<Item> = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if !line.starts_with('{') {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let get = |k: &str| v.get(k).and_then(|x| x.as_str()).map(|s| s.to_string());
        // Flat playlist entries carry `url`; fully resolved single videos carry `webpage_url`.
        let Some(url) = get("webpage_url").or_else(|| get("url")) else {
            continue;
        };
        if !url.starts_with("http") {
            continue;
        }
        let title = get("title").unwrap_or_else(|| "Untitled".to_string());
        let thumbnail = get("thumbnail").or_else(|| {
            v.get("thumbnails")
                .and_then(|t| t.as_array())
                .and_then(|a| a.last())
                .and_then(|t| t.get("url"))
                .and_then(|u| u.as_str())
                .map(|s| s.to_string())
        });
        let playlist = get("playlist_title").or_else(|| get("playlist"));
        items.push(Item {
            id: first_id + items.len(),
            title,
            url,
            thumbnail,
            playlist,
        });
    }
    // Only treat it as a playlist when there is more than one entry; a single
    // video that happens to know its playlist should not get a sub-folder.
    if items.len() <= 1 {
        for it in &mut items {
            it.playlist = None;
        }
    }
    items
}

/// Make a string safe to use as a single folder name on Windows, macOS and Linux.
pub fn sanitize_dir_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').trim().to_string();
    let cleaned: String = cleaned.chars().take(100).collect();
    if cleaned.is_empty() {
        "playlist".to_string()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_with_total() {
        let l = parse_line("[[P]]512|1024|NA|1048576.0|65");
        assert_eq!(
            l,
            Line::Progress {
                percent: Some(50.0),
                speed: Some("1.0 MiB/s".into()),
                eta: Some("01:05".into())
            }
        );
    }

    #[test]
    fn progress_falls_back_to_estimate() {
        match parse_line("[[P]]250|None|1000|NA|NA") {
            Line::Progress { percent, speed, eta } => {
                assert_eq!(percent, Some(25.0));
                assert_eq!(speed, None);
                assert_eq!(eta, None);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn progress_unknown_total_has_no_percent() {
        match parse_line("[[P]]250|NA|NA|NA|NA") {
            Line::Progress { percent, .. } => assert_eq!(percent, None),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn file_error_post_other() {
        assert_eq!(
            parse_line("[[F]]/tmp/a b.mp3"),
            Line::File(PathBuf::from("/tmp/a b.mp3"))
        );
        assert_eq!(
            parse_line("ERROR: [youtube] abc: Video unavailable"),
            Line::Error("[youtube] abc: Video unavailable".into())
        );
        assert_eq!(parse_line("[[PP]]FFmpegExtractAudio|started"), Line::PostProcess);
        assert_eq!(parse_line("[info] something"), Line::Other);
    }

    #[test]
    fn eta_formats() {
        assert_eq!(human_eta(5.0), "00:05");
        assert_eq!(human_eta(3725.0), "1:02:05");
    }

    #[test]
    fn entries_playlist_and_single() {
        let out = r#"{"id":"a","title":"One","url":"https://www.youtube.com/watch?v=a","playlist_title":"My List"}
{"id":"b","title":"Two","url":"https://www.youtube.com/watch?v=b","playlist_title":"My List","thumbnails":[{"url":"https://i/1.jpg"},{"url":"https://i/2.jpg"}]}
not json
{"id":"c","title":"No url"}"#;
        let items = parse_entries(out, 10);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, 10);
        assert_eq!(items[1].id, 11);
        assert_eq!(items[0].playlist.as_deref(), Some("My List"));
        assert_eq!(items[1].thumbnail.as_deref(), Some("https://i/2.jpg"));

        let single = r#"{"id":"a","title":"Solo","webpage_url":"https://soundcloud.com/x/y","playlist_title":"Whatever","thumbnail":"https://t/j.jpg"}"#;
        let items = parse_entries(single, 0);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].playlist, None);
        assert_eq!(items[0].url, "https://soundcloud.com/x/y");
    }

    #[test]
    fn sanitize() {
        assert_eq!(sanitize_dir_name("AC/DC: Live?"), "AC_DC_ Live_");
        assert_eq!(sanitize_dir_name("  ... "), "playlist");
        assert_eq!(sanitize_dir_name("ગુજરાતી ગીતો"), "ગુજરાતી ગીતો");
    }
}
