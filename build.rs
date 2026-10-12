fn main() {
    println!("cargo:rerun-if-changed=assets/app.ico");
    println!("cargo:rerun-if-changed=assets/suspended.ico");
    println!("cargo:rerun-if-changed=assets/app.manifest");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    // Icon IDs must match ICON_IDLE / ICON_SUSPENDED in src/tray.rs.
    winresource::WindowsResource::new()
        .set_icon_with_id("assets/app.ico", "1")
        .set_icon_with_id("assets/suspended.ico", "2")
        .set_manifest_file("assets/app.manifest")
        .compile()
        .expect("failed to compile Windows resources");
}
