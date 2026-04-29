fn main() {
    // On macOS, set rpath so the binary finds libfuse3 at runtime. macFUSE's
    // libfuse3.4.dylib has an absolute install name (/usr/local/lib/...) so
    // rpath is technically unused for it, but FUSE-T's variant uses
    // @rpath/libfuse3.4.dylib — keep both rpaths for parity.
    #[cfg(target_os = "macos")]
    {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/local/lib");
        println!("cargo:rustc-link-arg=-Wl,-rpath,/opt/homebrew/lib");
    }
}
