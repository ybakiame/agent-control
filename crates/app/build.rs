fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");

    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "Agent Control");
        res.set(
            "FileDescription",
            "Agent Control local-first engineering console",
        );
        res.set("LegalCopyright", "Apache-2.0");
        let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
        res.set("FileVersion", &version);
        res.set("ProductVersion", &version);
        if let Err(e) = res.compile() {
            panic!("failed to compile Windows resources: {e}");
        }
    }
}
