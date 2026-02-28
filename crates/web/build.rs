#[cfg(windows)]
fn main() {
    // 仅在主包构建时嵌入图标，避免作为依赖参与其它目标（例如桌面端）链接时引入资源冲突风险。
    if std::env::var_os("CARGO_PRIMARY_PACKAGE").is_none() {
        return;
    }

    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let icon_path = manifest_dir.join("../../apps/src-tauri/icons/icon.ico");

    println!("cargo:rerun-if-changed={}", icon_path.display());

    if !icon_path.is_file() {
        panic!("Windows icon not found: {}", icon_path.display());
    }

    let mut res = winres::WindowsResource::new();
    res.set_icon(icon_path.to_string_lossy().as_ref());
    res.compile()
        .expect("failed to compile Windows resources (icon)");
}

#[cfg(not(windows))]
fn main() {}

