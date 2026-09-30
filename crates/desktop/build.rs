fn main() {
    println!("cargo:rerun-if-env-changed=SIDEDOOR_VERSION");
    let version = std::env::var("SIDEDOOR_VERSION")
        .unwrap_or_else(|_| std::env::var("CARGO_PKG_VERSION").expect("package version"));
    println!("cargo:rustc-env=SIDEDOOR_BUILD_VERSION={version}");
    println!("cargo:rerun-if-changed=assets/icons/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set("ProductName", "Sidedoor")
            .set("FileDescription", "Sidedoor")
            .set_icon("assets/icons/icon.ico")
            .compile()
            .expect("compile Windows application resources");
    }
}
