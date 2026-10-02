/// <reference types="node" />
// The jsdeps cell's package graph as ES module code sees it: each assertion
// is checked twice, by tsc when the binary compiles (the annotated types)
// and by node when the test runs it.
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { type A, nameA, viaB } from "@tkfixture/cyc-a";
import { version } from "@tkfixture/host";
import { hostVersion } from "@tkfixture/plugin";
import { wrappedHostVersion } from "@tkfixture/wrapper";
import micromatch from "micromatch";
import pLimit from "p-limit";

const require = createRequire(import.meta.url);

// Two versions of one name: the project depends on host 1.0.0, the wrapper
// on host 2.0.0
const projectHost: "1.0.0" = version;
assert.equal(projectHost, "1.0.0");
assert.equal(require("picomatch/package.json").version, "4.0.2");
const micromatchRequire = createRequire(require.resolve("micromatch"));
assert.ok(micromatchRequire("picomatch/package.json").version.startsWith("2."));

// A peer split: the plugin is installed twice, once per host it sees as its
// peer, the project's and the wrapper's
const projectPlugin: "1.0.0" = hostVersion();
const wrapperPlugin: "2.0.0" = wrappedHostVersion();
assert.equal(projectPlugin, "1.0.0");
assert.equal(wrapperPlugin, "2.0.0");

// A cycle: cyc-a and cyc-b depend on each other, in types and at runtime
const cycle: A = { a: true, peer: { b: true, peer: { a: true } } };
assert.equal(cycle.peer?.peer?.a, true);
assert.equal(nameA, "a");
assert.equal(viaB(), "b>a");

// A package with undeclared transitive deps (braces, fill-range,
// to-regex-range, is-number, picomatch 2), and transitive @types: the
// options' type is @types/braces', which only @types/micromatch depends on
const braceOptions: NonNullable<Parameters<typeof micromatch.braces>[1]> = { expand: true };
assert.equal(micromatch.isMatch("a.js", "*.js"), true);
assert.deepEqual(micromatch.braces("x{1..3}", braceOptions), ["x1", "x2", "x3"]);

// An ES module package with an ES module dependency (yocto-queue)
const limit = pLimit(1);
assert.deepEqual(await Promise.all([1, 2].map((n) => limit(async () => n * 2))), [2, 4]);

// Undeclared transitive deps are not the project's to import
for (const name of ["braces", "fill-range", "yocto-queue", "@tkfixture/cyc-b", "@types/braces"]) {
	await assert.rejects(import(name), { code: "ERR_MODULE_NOT_FOUND" }, name);
}

console.log("esm: OK");
