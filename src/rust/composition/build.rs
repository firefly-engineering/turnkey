fn main() {
    // On macOS with fuse-t feature, tell the linker where to find libfuse3
    #[cfg(all(target_os = "macos", feature = "fuse-t"))]
    {
        // FUSE-T installs libraries here via Homebrew
        println!("cargo:rustc-link-search=/usr/local/lib");
        // Also check Homebrew on Apple Silicon
        println!("cargo:rustc-link-search=/opt/homebrew/lib");
    }
}
