fn main() {
    println!("cargo:rerun-if-changed=assets/wows-monitor-icon.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winres::WindowsResource::new();
        resource.set_icon("assets/wows-monitor-icon.ico");

        if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu") {
            let architecture = std::env::var("CARGO_CFG_TARGET_ARCH")
                .expect("Cargo provides the Windows target architecture");
            let tool_prefix = format!("{architecture}-w64-mingw32");
            resource
                .set_windres_path(&format!("{tool_prefix}-windres"))
                .set_ar_path(&format!("{tool_prefix}-ar"));
        }

        resource
            .compile()
            .expect("failed to embed the Windows executable icon");

        if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu") {
            let resource_object = std::path::Path::new(
                &std::env::var("OUT_DIR").expect("Cargo provides the build output directory"),
            )
            .join("resource.o");
            println!("cargo:rustc-link-arg={}", resource_object.display());
        }
    }
}
