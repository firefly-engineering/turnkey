/// <reference types="node" />
// Undeclared transitive deps are not the project's to import, in types
// either: tsc must fail to resolve each import below, or its expected
// error goes unused, which is an error itself.

// @ts-expect-error micromatch's dependency, not the project's
import type {} from "braces";
// @ts-expect-error @types/micromatch's dependency, not the project's
import type {} from "@types/braces";
// @ts-expect-error cyc-a's dependency, not the project's
import type {} from "@tkfixture/cyc-b";

export {};
