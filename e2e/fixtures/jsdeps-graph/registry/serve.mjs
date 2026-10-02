// A static npm registry serving the fixture's local packages, for
// regenerating pnpm-lock.yaml (see ../README.md).
//
// The packument of each package is built from its sources' package.json
// (packages/*/package.json), and its tarball is the one `pnpm pack` wrote
// to tarballs/. The lock records the tarballs' URLs and integrity; the
// fixture's flake reads the same files in place of those URLs, so this
// registry is never needed to build.
//
// Usage: node registry/serve.mjs [port]   (default 4873)

import crypto from "node:crypto";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";

const here = path.dirname(new URL(import.meta.url).pathname);
const port = Number(process.argv[2] ?? 4873);

// The tarball pnpm pack names a package's: `@scope/name` becomes
// `scope-name-<version>.tgz`
function tarballName(name, version) {
  const flat = name.startsWith("@") ? name.slice(1).replace("/", "-") : name;
  return `${flat}-${version}.tgz`;
}

function packuments() {
  const byName = {};
  const packagesDir = path.join(here, "packages");
  for (const dir of fs.readdirSync(packagesDir)) {
    const manifest = JSON.parse(fs.readFileSync(path.join(packagesDir, dir, "package.json"), "utf8"));
    const file = tarballName(manifest.name, manifest.version);
    const tarball = fs.readFileSync(path.join(here, "tarballs", file));
    const packument = (byName[manifest.name] ??= { name: manifest.name, versions: {}, "dist-tags": {} });
    packument.versions[manifest.version] = {
      ...manifest,
      dist: {
        tarball: `http://localhost:${port}/-/${file}`,
        integrity: `sha512-${crypto.createHash("sha512").update(tarball).digest("base64")}`,
        shasum: crypto.createHash("sha1").update(tarball).digest("hex"),
      },
    };
    packument["dist-tags"].latest = manifest.version;
  }
  return byName;
}

http
  .createServer((req, res) => {
    const url = decodeURIComponent(req.url);
    if (url.startsWith("/-/")) {
      const file = path.join(here, "tarballs", path.basename(url));
      if (fs.existsSync(file)) {
        res.end(fs.readFileSync(file));
        return;
      }
    }
    const packument = packuments()[url.slice(1)];
    if (packument) {
      res.setHeader("content-type", "application/json");
      res.end(JSON.stringify(packument));
      return;
    }
    res.statusCode = 404;
    res.end("{}");
  })
  .listen(port, () => console.log(`registry on http://localhost:${port}/`));
