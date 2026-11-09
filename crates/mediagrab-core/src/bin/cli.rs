//! Tiny command-line front end for the engine. Use it to test downloading
//! without the GUI:
//!
//!   cargo run -p mediagrab-core --bin mediagrab-cli -- --audio --out ./music <url> [<url>...]
//!   cargo run -p mediagrab-core --bin mediagrab-cli -- --video --height 720 --parallel 3 <url>

use mediagrab_core::{AudioFormat, Engine, Event, ItemState, Mode, Options};
use std::path::PathBuf;

fn usage() -> ! {
    eprintln!(
        "usage: mediagrab-cli [--audio|--video] [--format mp3|m4a|opus|flac] [--height N|best]\n\
         \x20                    [--out DIR] [--parallel N] [--cover-file] [--ytdlp PATH]\n\
         \x20                    [--extra \"yt-dlp args\"] <url>..."
    );
    std::process::exit(2)
}

#[tokio::main]
async fn main() {
    let mut opts = Options::default();
    let mut urls: Vec<String> = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--audio" => opts.mode = Mode::Audio,
            "--video" => opts.mode = Mode::Video,
            "--cover-file" => opts.save_cover_file = true,
            "--format" => {
                opts.audio_format = match args.next().as_deref() {
                    Some("mp3") => AudioFormat::Mp3,
                    Some("m4a") => AudioFormat::M4a,
                    Some("opus") => AudioFormat::Opus,
                    Some("flac") => AudioFormat::Flac,
                    _ => usage(),
                }
            }
            "--height" => {
                opts.max_height = match args.next().as_deref() {
                    Some("best") => None,
                    Some(n) => Some(n.parse().unwrap_or_else(|_| usage())),
                    None => usage(),
                }
            }
            "--out" => opts.out_dir = PathBuf::from(args.next().unwrap_or_else(|| usage())),
            "--parallel" => {
                opts.parallel = args.next().and_then(|n| n.parse().ok()).unwrap_or_else(|| usage())
            }
            "--ytdlp" => opts.ytdlp = args.next().unwrap_or_else(|| usage()),
            // e.g. --extra "--js-runtimes node --cookies-from-browser firefox"
            "--extra" => {
                let s = args.next().unwrap_or_else(|| usage());
                opts.extra_args.extend(s.split_whitespace().map(String::from));
            }
            "-h" | "--help" => usage(),
            _ => urls.push(a),
        }
    }
    if urls.is_empty() {
        usage();
    }

    let (mut rx, _cancel) = Engine::start(&tokio::runtime::Handle::current(), urls, opts);
    let mut titles: Vec<String> = Vec::new();
    let mut failed = 0;
    while let Some(ev) = rx.recv().await {
        match ev {
            Event::Status(s) => eprintln!("{s}"),
            Event::Error(e) => {
                failed += 1;
                eprintln!("ERROR {e}")
            }
            Event::ItemsFound(items) => {
                for it in &items {
                    eprintln!("  [{}] {}", it.id, it.title);
                }
                titles = items.into_iter().map(|i| i.title).collect();
            }
            Event::Progress { id, percent, speed, eta } => {
                if let Some(p) = percent {
                    // print only every ~10% to keep output readable
                    if (p as u32) % 10 == 0 {
                        eprintln!(
                            "  [{id}] {:>5.1}%  {}  eta {}",
                            p,
                            speed.unwrap_or_default(),
                            eta.unwrap_or_default()
                        );
                    }
                }
            }
            Event::State { id, state } => {
                let t = titles.get(id).cloned().unwrap_or_default();
                match state {
                    ItemState::Done(p) => eprintln!("DONE   [{id}] {t} -> {}", p.display()),
                    ItemState::Failed(e) => {
                        failed += 1;
                        eprintln!("FAILED [{id}] {t}: {e}")
                    }
                    ItemState::Processing => eprintln!("  [{id}] processing ..."),
                    _ => {}
                }
            }
            Event::Finished => break,
        }
    }
    std::process::exit(if failed > 0 { 1 } else { 0 });
}
