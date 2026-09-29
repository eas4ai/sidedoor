fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=assets/windows.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set("ProductName", "Sidedoor")
            .set("FileDescription", "Sidedoor")
            .set_icon("assets/icon.ico")
            .set_manifest_file("assets/windows.manifest")
            .compile()
            .expect("compile Windows application resources");
    }
}
