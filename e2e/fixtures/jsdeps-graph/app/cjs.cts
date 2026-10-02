/// <reference types="node" />
// The jsdeps cell's package graph as CommonJS code sees it: each assertion
// is checked twice, by tsc when the binary compiles (the annotated types)
// and by node when the test runs it.
import assert = require("node:assert/strict");
import cycA = require("@tkfixture/cyc-a");
import host = require("@tkfixture/host");
import plugin = require("@tkfixture/plugin");
import wrapper = require("@tkfixture/wrapper");
import micromatch = require("micromatch");

async function main(): Promise<void> {
	// Two versions of one name: the project depends on host 1.0.0, the
	// wrapper on host 2.0.0
	const projectHost: "1.0.0" = host.version;
	assert.equal(projectHost, "1.0.0");
	assert.equal(require("picomatch/package.json").version, "4.0.2");
	const micromatchRequire = require("node:module").createRequire(require.resolve("micromatch"));
	assert.ok(micromatchRequire("picomatch/package.json").version.startsWith("2."));

	// A peer split: the plugin is installed twice, once per host it sees as
	// its peer, the project's and the wrapper's
	const projectPlugin: "1.0.0" = plugin.hostVersion();
	const wrapperPlugin: "2.0.0" = wrapper.wrappedHostVersion();
	assert.equal(projectPlugin, "1.0.0");
	assert.equal(wrapperPlugin, "2.0.0");

	// A cycle: cyc-a and cyc-b depend on each other, in types and at runtime
	const cycle: cycA.A = { a: true, peer: { b: true, peer: { a: true } } };
	assert.equal(cycle.peer?.peer?.a, true);
	assert.equal(cycA.viaB(), "b>a");

	// A package with undeclared transitive deps, and transitive @types
	const braceOptions: NonNullable<Parameters<typeof micromatch.braces>[1]> = { expand: true };
	assert.equal(micromatch.isMatch("a.ts", "*.ts"), true);
	assert.deepEqual(micromatch.braces("x{1..3}", braceOptions), ["x1", "x2", "x3"]);

	// An ES module package from CommonJS code
	const { default: pLimit } = await import("p-limit");
	const limit = pLimit(1);
	assert.deepEqual(await Promise.all([1, 2].map((n) => limit(async () => n * 2))), [2, 4]);

	// Undeclared transitive deps are not the project's to require
	for (const name of ["braces", "fill-range", "yocto-queue", "@tkfixture/cyc-b", "@types/braces"]) {
		assert.throws(() => require.resolve(name), { code: "MODULE_NOT_FOUND" }, name);
	}

	console.log("cjs: OK");
}

main().catch((err) => {
	console.error(err);
	process.exit(1);
});
