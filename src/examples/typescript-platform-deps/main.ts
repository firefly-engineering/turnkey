// TypeScript example whose npm dependency has a platform-specific optional
// dependency: chokidar uses fsevents, which npm installs on macOS only. The
// jsdeps cell resolves it per platform, so the build links fsevents into
// node_modules on macOS and not on Linux.
console.log("chokidar is in node_modules, with fsevents on macOS");
