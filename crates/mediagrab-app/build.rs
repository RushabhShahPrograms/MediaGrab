// Embeds assets/icon.ico into the Windows .exe (taskbar, Explorer, Alt-Tab).
fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "MediaGrab");
        res.set("FileDescription", "MediaGrab");
        res.compile().expect("failed to embed icon");
    }
}
