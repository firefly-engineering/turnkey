fn main() {
    // On macOS with the libfuse3 backend, tell the linker where to find libfuse3.
    // macFUSE installs libfuse3.4.dylib to /usr/local/lib on both Intel and Apple Silicon.
    #[cfg(all(target_os = "macos", feature = "fuse-t"))]
    {
        println!("cargo:rustc-link-search=/usr/local/lib");
        // Apple Silicon Homebrew prefix, kept as a fallback for non-macFUSE setups.
        println!("cargo:rustc-link-search=/opt/homebrew/lib");
    }
}
