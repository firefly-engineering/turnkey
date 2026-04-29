fn main() {
    // On macOS, link against macFUSE's libfuse3 at /usr/local/lib. The
    // FSKit code path is opted into at mount time via `-o backend=fskit`
    // (see fuse_t/backend.rs). Without that option, libfuse3 falls back to
    // the legacy `mount_macfuse` kext helper, which is blocked by
    // syspolicyd on Apple Silicon Tahoe without Reduced Security mode.
    #[cfg(all(target_os = "macos", feature = "fuse-t"))]
    {
        println!("cargo:rustc-link-search=/usr/local/lib");
        // Apple Silicon Homebrew prefix, fallback for non-macFUSE setups.
        println!("cargo:rustc-link-search=/opt/homebrew/lib");
    }
}
