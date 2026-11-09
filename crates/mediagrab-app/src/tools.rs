//! Makes the bundled yt-dlp / ffmpeg / deno visible to the engine.
//!
//! The installers ship these programs next to the app. The engine starts `yt-dlp`
//! by name, and yt-dlp finds `ffmpeg` and `deno` the same way, so the simplest and
//! most robust integration is to put the bundled folder at the FRONT of `PATH`.
//! If nothing is bundled (e.g. `cargo run` during development) this does nothing
//! and the tools installed on the system are used as before.

use std::path::PathBuf;

pub fn add_bundled_tools_to_path() {
    let Ok(exe) = std::env::current_exe() else { return };
    let Some(dir) = exe.parent() else { return };

    let candidates = [
        dir.join("tools"),                 // Windows installer / portable zip
        dir.join("../Resources/tools"),    // macOS: MediaGrab.app/Contents/Resources/tools
        dir.join("../lib/mediagrab/tools"), // Linux: /usr/bin -> /usr/lib/mediagrab/tools (.deb, AppImage)
    ];
    let mut paths: Vec<PathBuf> = candidates.into_iter().filter(|p| p.is_dir()).collect();
    if paths.is_empty() {
        return;
    }
    if let Some(old) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&old));
    }
    if let Ok(new_path) = std::env::join_paths(paths) {
        // SAFETY: called first thing in main(), before any other thread exists.
        #[allow(unused_unsafe)]
        unsafe {
            std::env::set_var("PATH", new_path)
        };
    }
}
