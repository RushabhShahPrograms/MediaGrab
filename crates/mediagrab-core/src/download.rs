//! Step 2: download ONE item by running ONE yt-dlp process.
//!
//! Why shell out to yt-dlp instead of using a Rust crate that re-implements YouTube?
//! YouTube changes its internals every few weeks; yt-dlp is updated within days.
//! Driving the real binary keeps this app working by simply running `yt-dlp -U`.
//! It also gives us SoundCloud (and ~1800 other sites) for free.

use crate::parse::{parse_line, sanitize_dir_name, Line, FILE_MARK, POST_MARK, PROGRESS_MARK};
use crate::resolve::spawn_error;
use crate::types::{Event, Item, ItemState, Mode, Options};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, watch};

/// Build the yt-dlp argument list for one item. Pure function => unit tested.
pub fn build_args(opts: &Options, item: &Item) -> Vec<String> {
    let dir = output_dir(opts, item);
    let template = format!("{}/%(title)s.%(ext)s", dir.display());

    let mut a: Vec<String> = Vec::new();
    let mut push = |s: &str| a.push(s.to_string());

    // --- common ---------------------------------------------------------
    push("--no-warnings");
    push("--no-colors");
    push("--newline"); // one progress update per line (easy to parse)
    push("--progress"); // keep progress output even though --print implies --quiet
    push("--no-playlist"); // this URL is ONE item; never expand it again
    push("--windows-filenames"); // strip characters that are illegal on Windows, keep unicode titles
    push("--trim-filenames");
    push("150");
    push("--embed-metadata"); // writes the title (and artist etc.) into the file's tags
    push("--progress-template");
    push(&format!(
        "download:{PROGRESS_MARK}%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s"
    ));
    push("--progress-template");
    push(&format!(
        "postprocess:{POST_MARK}%(progress.postprocessor)s|%(progress.status)s"
    ));
    push("--print");
    push(&format!("after_move:{FILE_MARK}%(filepath)s"));
    push("-o");
    push(&template);

    // --- mode specific --------------------------------------------------
    match opts.mode {
        Mode::Audio => {
            push("-f");
            push("bestaudio/best");
            push("-x"); // extract audio
            push("--audio-format");
            push(opts.audio_format.as_arg());
            push("--audio-quality");
            push("0"); // best VBR quality
            push("--embed-thumbnail"); // cover art inside the file
            push("--convert-thumbnails");
            push("jpg"); // webp cover art is not supported by most players
        }
        Mode::Video => {
            // Prefer mp4/m4a streams (h264+aac) so the merged file plays everywhere.
            let sort = match opts.max_height {
                Some(h) => format!("res:{h},ext:mp4:m4a"),
                None => "ext:mp4:m4a".to_string(),
            };
            push("-S");
            push(&sort);
            push("-f");
            push("bv*+ba/b");
            push("--merge-output-format");
            push("mp4");
            // Video = title only. No thumbnail anywhere.
            push("--no-write-thumbnail");
            push("--no-embed-thumbnail");
        }
    }

    // User-supplied extras go last so they can override anything above.
    a.extend(opts.extra_args.iter().cloned());
    a.push(item.url.clone());
    a
}

pub fn output_dir(opts: &Options, item: &Item) -> PathBuf {
    match &item.playlist {
        Some(p) => opts.out_dir.join(sanitize_dir_name(p)),
        None => opts.out_dir.clone(),
    }
}

/// Optional second pass: save the thumbnail as `<title>.jpg` next to the audio file.
fn cover_args(opts: &Options, item: &Item) -> Vec<String> {
    let dir = output_dir(opts, item);
    let mut v: Vec<String> = vec![
        "--no-warnings".into(),
        "--no-playlist".into(),
        "--skip-download".into(),
        "--write-thumbnail".into(),
        "--convert-thumbnails".into(),
        "jpg".into(),
        "--windows-filenames".into(),
        "--trim-filenames".into(),
        "150".into(),
        "-o".into(),
        format!("{}/%(title)s.%(ext)s", dir.display()),
    ];
    v.extend(opts.extra_args.iter().cloned());
    v.push(item.url.clone());
    v
}

/// Resolves when the cancel flag becomes true. If the sender is dropped we simply never cancel.
pub(crate) async fn cancelled(rx: &mut watch::Receiver<bool>) {
    loop {
        if *rx.borrow_and_update() {
            return;
        }
        if rx.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

/// Forward every line of a pipe into a channel. Ends when the pipe closes (process exited).
fn pump<R: AsyncRead + Unpin + Send + 'static>(r: R, tx: mpsc::UnboundedSender<String>) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(r).lines();
        while let Ok(Some(l)) = lines.next_line().await {
            if tx.send(l).is_err() {
                break;
            }
        }
    });
}

/// Download one item, sending `Event`s as it goes. Returns the final state too.
pub async fn download_item(
    opts: &Options,
    item: &Item,
    events: &mpsc::UnboundedSender<Event>,
    cancel: &mut watch::Receiver<bool>,
) -> ItemState {
    let id = item.id;
    let send_state = |s: ItemState| {
        let _ = events.send(Event::State { id, state: s });
    };
    send_state(ItemState::Downloading);

    let mut child = match Command::new(&opts.ytdlp)
        .args(build_args(opts, item))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true) // if we are dropped for any reason, the child dies with us
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let st = ItemState::Failed(spawn_error(&opts.ytdlp, e));
            send_state(st.clone());
            return st;
        }
    };

    // yt-dlp can write progress to stdout or stderr depending on flags/version,
    // so we read BOTH into one channel and parse every line the same way.
    let (ltx, mut lrx) = mpsc::unbounded_channel::<String>();
    if let Some(o) = child.stdout.take() {
        pump(o, ltx.clone());
    }
    if let Some(e) = child.stderr.take() {
        pump(e, ltx.clone());
    }
    drop(ltx); // channel closes once both pipes are closed

    let mut final_path: Option<PathBuf> = None;
    let mut last_error: Option<String> = None;
    let mut processing_sent = false;
    let mut saw_complete = false; // a progress line reached 100%

    loop {
        tokio::select! {
            line = lrx.recv() => {
                let Some(line) = line else { break };
                match parse_line(&line) {
                    Line::Progress { percent, speed, eta } => {
                        if matches!(percent, Some(p) if p >= 99.9) {
                            saw_complete = true;
                        }
                        let _ = events.send(Event::Progress { id, percent, speed, eta });
                    }
                    Line::PostProcess => {
                        // yt-dlp reports every post-processor start/finish; tell the UI only once,
                        // and only after the download itself reached 100% (stdout and stderr are
                        // read as two pipes, so line order between them is not guaranteed).
                        if !processing_sent && saw_complete {
                            processing_sent = true;
                            send_state(ItemState::Processing);
                        }
                    }
                    Line::File(p) => final_path = Some(p),
                    Line::Error(e) => last_error = Some(e),
                    Line::Other => {}
                }
            }
            _ = cancelled(cancel) => {
                let _ = child.kill().await;
                send_state(ItemState::Cancelled);
                return ItemState::Cancelled;
            }
        }
    }

    let status = child.wait().await;
    let ok = matches!(&status, Ok(s) if s.success());
    let state = if ok {
        let path = final_path.unwrap_or_else(|| output_dir(opts, item));
        if opts.mode == Mode::Audio && opts.save_cover_file {
            save_cover_file(opts, item).await;
        }
        ItemState::Done(path)
    } else {
        let msg = last_error
            .or_else(|| status.err().map(|e| e.to_string()))
            .unwrap_or_else(|| "yt-dlp exited with an error".to_string());
        ItemState::Failed(msg)
    };
    send_state(state.clone());
    state
}

async fn save_cover_file(opts: &Options, item: &Item) {
    // Best effort: a failure here must not turn a successful download into a failed one.
    let _ = Command::new(&opts.ytdlp)
        .args(cover_args(opts, item))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .status()
        .await;
}

#[allow(dead_code)]
fn _assert_path_is_used(_: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::AudioFormat;

    fn item(playlist: Option<&str>) -> Item {
        Item {
            id: 0,
            title: "T".into(),
            url: "https://example.com/v".into(),
            thumbnail: None,
            playlist: playlist.map(|s| s.to_string()),
        }
    }

    fn has_pair(args: &[String], a: &str, b: &str) -> bool {
        args.windows(2).any(|w| w[0] == a && w[1] == b)
    }

    #[test]
    fn audio_args_embed_thumbnail_and_title() {
        let o = Options {
            mode: Mode::Audio,
            audio_format: AudioFormat::Mp3,
            ..Options::default()
        };
        let a = build_args(&o, &item(None));
        assert!(a.contains(&"-x".to_string()));
        assert!(a.contains(&"--embed-thumbnail".to_string()));
        assert!(a.contains(&"--embed-metadata".to_string()));
        assert!(has_pair(&a, "--audio-format", "mp3"));
        assert_eq!(a.last().unwrap(), "https://example.com/v");
    }

    #[test]
    fn video_args_have_no_thumbnail() {
        let o = Options {
            mode: Mode::Video,
            max_height: Some(720),
            ..Options::default()
        };
        let a = build_args(&o, &item(None));
        assert!(a.contains(&"--no-write-thumbnail".to_string()));
        assert!(a.contains(&"--no-embed-thumbnail".to_string()));
        assert!(!a.contains(&"--embed-thumbnail".to_string()));
        assert!(!a.contains(&"-x".to_string()));
        assert!(has_pair(&a, "-S", "res:720,ext:mp4:m4a"));
        assert!(has_pair(&a, "--merge-output-format", "mp4"));
    }

    #[test]
    fn playlist_gets_subfolder() {
        let o = Options {
            out_dir: PathBuf::from("/music"),
            ..Options::default()
        };
        assert_eq!(output_dir(&o, &item(Some("Best/Of"))), PathBuf::from("/music/Best_Of"));
        assert_eq!(output_dir(&o, &item(None)), PathBuf::from("/music"));
    }
}
