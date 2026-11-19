fn main() {
    // Resource 1 is the icon GPUI shows in the title bar and taskbar.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("assets/app.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("icon resource compiles");
    }
}
