//! Graphical startup compatibility. No sandbox or TLS workarounds.
pub fn setup() {
    #[cfg(target_os = "linux")]
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        for name in [
            "WEBKIT_DISABLE_DMABUF_RENDERER",
            "WEBKIT_DISABLE_COMPOSITING_MODE",
        ] {
            if std::env::var_os(name).is_none() {
                std::env::set_var(name, "1");
            }
        }
    }
}
