fn main() {
    println!("cargo:rerun-if-changed=assets/app-icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/app-icon.ico")
            .set("ProductName", "PrintHelper")
            .set("FileDescription", "PrintHelper")
            .set(
                "OriginalFilename",
                &format!("PrintHelper-v{}.exe", env!("CARGO_PKG_VERSION")),
            )
            .compile_for("print-helper")
            .expect("Compile the PrintHelper icon and version resources");
    }
}
