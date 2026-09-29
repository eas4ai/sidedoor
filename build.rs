fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set("ProductName", "Sidedoor")
            .set("FileDescription", "Sidedoor")
            .set_icon("assets/icon.ico")
            .compile()
            .expect("compile Windows application resources");
    }
}
