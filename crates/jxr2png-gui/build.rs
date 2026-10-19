fn main() {
    // Embed the icon in the Windows executable, where Explorer, the taskbar
    // and the "Open with" menu read it from.
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/icon.ico")
            .compile()
            .expect("embed the Windows icon");
    }
}
