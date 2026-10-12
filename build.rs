fn main() {
    for path in [
        "assets/app.ico",
        "assets/suspended.ico",
        "assets/app.manifest",
        "assets/dialogs.rc",
        "assets/resource.h",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    // Icon IDs must match IDI_APP / IDI_SUSPENDED in assets/resource.h.
    winresource::WindowsResource::new()
        .set_icon_with_id("assets/app.ico", "1")
        .set_icon_with_id("assets/suspended.ico", "2")
        .set_manifest_file("assets/app.manifest")
        .append_rc_content("#include \"assets/dialogs.rc\"")
        .add_toolkit_include(true)
        .compile()
        .expect("failed to compile Windows resources");
}
