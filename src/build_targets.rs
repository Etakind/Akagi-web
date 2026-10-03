//! One reviewed target inventory shared with packaging and workflows.
#[derive(serde::Deserialize)]
pub struct Target {
    pub target: String,
    pub os: String,
    pub arch: String,
    pub slug: String,
    pub cft: String,
}
pub fn all() -> &'static [Target] {
    static TARGETS: std::sync::OnceLock<Vec<Target>> = std::sync::OnceLock::new();
    TARGETS.get_or_init(|| {
        serde_json::from_str(include_str!("../build/targets.json"))
            .expect("reviewed target inventory")
    })
}
pub fn current() -> Option<&'static Target> {
    all()
        .iter()
        .find(|t| t.os == std::env::consts::OS && t.arch == std::env::consts::ARCH)
}
