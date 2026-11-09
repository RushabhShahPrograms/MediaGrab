# MediaGrab

A desktop app (Rust + [GPUI](https://www.gpui.rs/)) for creators to download:

* **YouTube video** (title only, no thumbnail) as `.mp4`
* **YouTube audio / music** as MP3, M4A, Opus or FLAC, **with the title in the tags and the thumbnail embedded as cover art**
* **SoundCloud music**, same as audio above
* from a **single link or a whole playlist / SoundCloud set**, downloading **several items in parallel**

> Only download content you own or have permission to download. Downloading from YouTube may
> violate YouTube's Terms of Service, and copyright applies to what you download.

---

## 1. How it is built (read this first)

```
crates/
  mediagrab-core/        <- the engine. No UI code. Fully testable from a terminal.
    types.rs             plain data: Options, Item, ItemState, Event
    parse.rs             pure functions: parse yt-dlp output          (unit tested)
    resolve.rs           link -> list of items (playlist expansion)
    download.rs          ONE yt-dlp process for ONE item, parse progress, handle cancel
    engine.rs            queue + Semaphore (parallelism) + Event channel + cancel
    bin/cli.rs           command-line front end for testing the engine
  mediagrab-app/         <- GPUI front end (window, buttons, list). Thin: just applies Events.
scripts/local_test.sh    offline end-to-end smoke test (no internet needed)
```

```
 pasted link(s) --> resolve --> [Item, Item, Item ...]
                                     |  one tokio task per item
                                     v
                         Semaphore(parallel = N permits)
                           |      |      |      (N yt-dlp processes at once)
                           v      v      v
                        Events (progress, state) --one channel--> GPUI view --> screen
```

### Which packages, and why

| Need | Choice | Why |
|---|---|---|
| YouTube + SoundCloud extraction | **`yt-dlp` binary**, driven with `tokio::process` | YouTube changes constantly; yt-dlp is updated within days. It also supports SoundCloud (and ~1800 other sites) with the same flags, so one backend serves both. |
| Audio conversion, merging, cover-art embedding | **`ffmpeg`** (called by yt-dlp) | Required for mp3/m4a/flac/opus conversion, merging video+audio into mp4 and embedding thumbnails. |
| Parallelism | **`tokio`** (`JoinSet`, `Semaphore`, `mpsc`, `watch`) | One task per item, a semaphore caps concurrency, channels carry events/cancel. |
| Parsing yt-dlp's JSON | **`serde_json`** | Only dependency besides tokio in the core. |
| UI | **`gpui` 0.2.2** + **`gpui-component` 0.5.1** | GPUI has no built-in text input or buttons; gpui-component supplies `Input`, `Button` and the required `Root`. |

Crates I looked at but did **not** use: `yt-dlp` (async wrapper that auto-downloads yt-dlp/ffmpeg),
`ytdown` (pure-Rust re-implementation), `libytdlr`. Pure-Rust re-implementations of YouTube
extraction break whenever YouTube changes; wrappers around the real binary don't. If you later want
auto-downloading of yt-dlp/ffmpeg, the `yt-dlp` crate does that and could replace `download.rs`.
There is no separate "soundcloud" crate needed because yt-dlp handles SoundCloud.

---

## 2. Prerequisites

1. **Rust** (latest stable; GPUI needs a recent toolchain): <https://rustup.rs>
2. **yt-dlp** (keep it updated: `yt-dlp -U`)
   * Windows: `winget install yt-dlp`   * macOS: `brew install yt-dlp`   * Linux: `pipx install yt-dlp` (or your package manager / the official binary)
3. **ffmpeg**
   * Windows: `winget install ffmpeg`   * macOS: `brew install ffmpeg`   * Linux: `sudo apt install ffmpeg`
4. **A JavaScript runtime for YouTube (Deno recommended).** Recent yt-dlp versions need an external JS
   runtime to solve YouTube's challenges; without it YouTube downloads can fail or miss formats.
   Install Deno from <https://docs.deno.com/runtime/getting_started/installation/>. Deno is enabled by default
   in yt-dlp. To use Node instead: set `MEDIAGRAB_YTDLP_ARGS="--js-runtimes node"` (see Troubleshooting).
   The official yt-dlp executables already bundle the challenge-solver scripts; if you installed yt-dlp
   through pip, also `pip install yt-dlp-ejs`. Details: the "EJS" page of the yt-dlp wiki.
5. **Platform libraries for GPUI (only needed for the GUI, not for the CLI).** Follow the GPUI/Zed
   build prerequisites for your OS: Windows needs Visual Studio Build Tools with the Windows SDK;
   macOS needs Xcode command line tools; Linux needs the usual graphics/windowing dev packages
   (Wayland/X11, xkbcommon, fontconfig, Vulkan).

Check your tools:

```bash
yt-dlp --version && ffmpeg -version | head -1 && cargo --version
```

---

## 3. Test in this order

### Step A: engine only, offline (no GUI, no internet)

```bash
./scripts/local_test.sh          # Linux/macOS/WSL; needs yt-dlp, ffmpeg, python3
cargo test -p mediagrab-core     # unit tests for parsing + yt-dlp argument building
```

The script serves generated files on localhost and downloads them through the real engine, so you
see queueing, parallel progress, conversion and tagging working.

### Step B: engine against the real sites (CLI)

```bash
# one track, SoundCloud
cargo run -p mediagrab-core --bin mediagrab-cli -- --audio --out ./music "https://soundcloud.com/ARTIST/TRACK"

# a YouTube playlist as MP3, 4 at a time
cargo run -p mediagrab-core --bin mediagrab-cli -- --audio --format mp3 --parallel 4 --out ./music \
  "https://www.youtube.com/playlist?list=PLAYLIST_ID"

# a single YouTube video, max 720p
cargo run -p mediagrab-core --bin mediagrab-cli -- --video --height 720 --out ./videos "https://www.youtube.com/watch?v=VIDEO_ID"
```

Other flags: `--format mp3|m4a|opus|flac`, `--height N|best`, `--cover-file`,
`--extra "--js-runtimes node"`, `--ytdlp /path/to/yt-dlp`.

### Step C: the GUI

```bash
cargo run -p mediagrab-app --release      # first build compiles GPUI: expect a long compile
```

1. Paste one or more links (space separated). A playlist link downloads every entry.
2. Pick **Music / audio** or **Video**, then format or quality.
3. Choose the **Save to** folder (type or paste a path; default is `~/Downloads/MediaGrab`).
4. Press **Download**. Rows show queued / % / speed / ETA / done / failed. **Cancel** stops everything and kills the yt-dlp processes.

---

## 4. Behaviour details

* **Audio:** best audio stream, converted to your chosen format at best quality; title (plus artist/other metadata) written to tags; thumbnail converted to JPG and **embedded as cover art**. "Also save cover .jpg" additionally writes `<title>.jpg` next to the file.
* **Video:** best video+audio up to your chosen height, preferring mp4/m4a streams, merged to `.mp4`. Title is in the filename and metadata. **No thumbnail** is downloaded or embedded.
* **Playlists:** items go into a sub-folder named after the playlist. A watch link that also contains `&list=` downloads only that one video; paste the `.../playlist?list=` link for the whole list.
* **Parallel:** the "Parallel" setting is the number of simultaneous yt-dlp processes (1-12). Start with 3-4; too many can trigger rate limiting.
* **Progress for video** can run 0-100% twice (video stream, then audio stream) before merging.
* **Existing files** are skipped by yt-dlp ("already downloaded").
* **Failures are per item:** one private/deleted entry does not stop the playlist.

---

## 5. Troubleshooting

| Symptom | Fix |
|---|---|
| `could not start 'yt-dlp': not found` | Install yt-dlp and make sure it is on `PATH` (restart the terminal/app). |
| YouTube errors like "video not available", missing formats, HTTP 403 | `yt-dlp -U`; install **Deno**; if you use Node: `MEDIAGRAB_YTDLP_ARGS="--js-runtimes node"`. |
| "Sign in to confirm you're not a bot" | Use your browser's cookies: `MEDIAGRAB_YTDLP_ARGS="--cookies-from-browser firefox"` (or chrome/edge). |
| Audio is not converted / no cover art | ffmpeg missing: install it and check `ffmpeg -version`. |
| SoundCloud track "not available" / only 30s preview | Some tracks are geo-restricted or Go+-only; yt-dlp can only fetch what the site serves you. |
| GUI build errors | Make sure `gpui = 0.2.2` and `gpui-component = 0.5.1` are kept as a pair, Rust is up to date, and your OS prerequisites for GPUI are installed. |

Set the env var before launching, e.g. PowerShell:
`$env:MEDIAGRAB_YTDLP_ARGS="--js-runtimes node"; cargo run -p mediagrab-app --release`
Linux/macOS: `MEDIAGRAB_YTDLP_ARGS="--js-runtimes node" cargo run -p mediagrab-app --release`

---

## 6. What was verified, and what was not

| Part | Status |
|---|---|
| `mediagrab-core` compiles, 10 unit tests pass | verified |
| Parallel batch (queueing, per-item progress, conversion to FLAC/MP3, title tag), failing link handled per item | verified end to end against a local HTTP server |
| Video mode produces `.mp4` | verified (local file) |
| Cancel kills the running yt-dlp and cancels queued items, no leftover processes | verified |
| Real YouTube / SoundCloud downloads, playlist sub-folders, cover-art embedding | **not verified** (no access to those sites from where this was built) |
| `mediagrab-app` (GPUI) | **written but not compiled** (the build environment lacked a recent Rust toolchain and GPUI's system libraries). Expect to fix a few small API mismatches on first build. The app is ~460 lines in one file and the engine does not depend on it. |

## 7. Ideas for next steps

* Native folder picker (GPUI's path prompt) instead of the text field
* Show thumbnails in the list (needs an HTTP client registered with GPUI)
* Drag-and-drop links, clipboard watcher
* Per-item retry button, pause/resume
* Auto-update yt-dlp on start (`yt-dlp -U`) or switch `download.rs` to the `yt-dlp` crate to auto-fetch binaries
