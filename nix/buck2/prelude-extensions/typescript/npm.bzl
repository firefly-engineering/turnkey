# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""npm package instances, the rules of the jsdeps cell's root package.

The jsdeps cell (docs/adr/0012-jsdeps-separates-package-contents-from-the-instance-graph.md)
declares one npm_instance per pnpm snapshot, one npm_component per
dependency cycle with an npm_member per instance in it, and an alias per
direct dependency.

An instance's output is a real directory, node_modules/<name>/, copied
from its package's files with every link dereferenced: node and tsc find a
package's dependencies by walking node_modules up from its realpath, so
the package can't stay a link into /nix/store. Each dependency is a
relative symlink beside it, node_modules/<import name>, into the
dependency instance's output. The output doesn't have a content-based path
and the links don't make the dependency an input: a link is keyed by its
path, so a dependency's content change doesn't re-run its dependents. A
consumer links its direct instances' package directories into its own
node_modules and carries every instance output of their closure as hidden
inputs, so all of it is there when node or tsc runs.

A cycle's instances share one output, a directory per member, with their
links to each other inside it; the target graph stays acyclic.
"""

def _closure_artifacts(value: Artifact):
    return value

# The outputs of the instances (and components) in a package's closure
NpmClosureTSet = transitive_set(args_projections = {"artifacts": _closure_artifacts})

NpmPackageInfo = provider(
    doc = "An npm package instance: what a consumer links into its node_modules.",
    fields = {
        # Artifact: the instance's node_modules/<name> directory
        "package_dir": provider_field(typing.Any, default = None),
        # NpmClosureTSet: its output, and those of every instance it depends on
        "closure": provider_field(typing.Any, default = None),
    },
)

NpmComponentInfo = provider(
    doc = "A dependency cycle's instances, sharing one output.",
    fields = {
        # dict[str, Artifact]: each member's node_modules/<name> directory
        "members": provider_field(typing.Any, default = {}),
        "closure": provider_field(typing.Any, default = None),
    },
)

# Lays out an output directory from operations, each three arguments:
#   copy <path> <dir>     a dereferencing copy of dir at <out>/<path>
#   link <path> <target>  a symlink at <out>/<path> to target, as given
_LAYOUT = """
set -eu
out="$1"
shift
mkdir -p "$out"
while [ "$#" -gt 0 ]; do
    dst="$out/$2"
    mkdir -p "$(dirname "$dst")"
    case "$1" in
        copy)
            cp -RL "$3" "$dst"
            chmod -R u+w "$dst"
            ;;
        link)
            ln -s "$3" "$dst"
            ;;
        *)
            echo "npm layout: unknown operation $1" >&2
            exit 1
            ;;
    esac
    shift 3
done
"""

def _layout(out: Artifact) -> cmd_args:
    return cmd_args("/bin/sh", "-c", _LAYOUT, "npm_layout", out.as_output())

def _link_target(out: Artifact, link: str, target: Artifact) -> cmd_args:
    """The relative path a symlink at <out>/<link> takes to reach target,
    which doesn't become an input of the action: the link is keyed by its
    path only."""
    up = "../" * link.count("/")
    return cmd_args(target, relative_to = out, format = up + "{}", ignore_artifacts = True)

def _files_dir(dep: Dependency) -> Artifact:
    return dep[DefaultInfo].default_outputs[0]

def _deps_of(ctx: AnalysisContext, member: str | None = None) -> dict[str, Dependency]:
    """A target's dependencies by import name, its platform's optional ones
    included: a component's are per member"""
    deps = dict(ctx.attrs.deps if member == None else ctx.attrs.deps.get(member, {}))
    optional = ctx.attrs.optional_deps if member == None else ctx.attrs.optional_deps.get(member, {})
    deps.update(optional)
    return deps

def _npm_instance_impl(ctx: AnalysisContext) -> list[Provider]:
    out = ctx.actions.declare_output("i", dir = True, has_content_based_path = False)
    package_path = "node_modules/" + ctx.attrs.package
    cmd = _layout(out)
    cmd.add("copy", package_path, _files_dir(ctx.attrs.files))
    children = []
    for name, dep in sorted(_deps_of(ctx).items()):
        info = dep[NpmPackageInfo]
        link = "node_modules/" + name
        cmd.add("link", link, _link_target(out, link, info.package_dir))
        children.append(info.closure)
    ctx.actions.run(cmd, category = "npm_instance", identifier = ctx.label.name)
    closure = ctx.actions.tset(NpmClosureTSet, value = out, children = children)
    return [
        DefaultInfo(default_output = out),
        NpmPackageInfo(package_dir = out.project(package_path), closure = closure),
    ]

npm_instance = rule(
    impl = _npm_instance_impl,
    attrs = {
        "package": attrs.string(doc = "The package's npm name, its directory in node_modules"),
        "files": attrs.dep(doc = "The package's contents (vendor/<name>@<version>:files)"),
        "deps": attrs.dict(
            attrs.string(),
            attrs.dep(providers = [NpmPackageInfo]),
            default = {},
            doc = "The instances it depends on, by the name it imports each as",
        ),
        "optional_deps": attrs.dict(
            attrs.string(),
            attrs.dep(providers = [NpmPackageInfo]),
            default = {},
            doc = "The optional dependencies npm installs on the platform, likewise",
        ),
    },
    doc = "One npm package instance: its package directory, and its dependencies linked beside it.",
)

def _npm_component_impl(ctx: AnalysisContext) -> list[Provider]:
    out = ctx.actions.declare_output("c", dir = True, has_content_based_path = False)
    cmd = _layout(out)
    package_paths = {
        member: "{}/node_modules/{}".format(member, package)
        for member, package in ctx.attrs.members.items()
    }
    children = []
    for member in sorted(ctx.attrs.members):
        cmd.add("copy", package_paths[member], _files_dir(ctx.attrs.files[member]))
        for name, other in sorted(ctx.attrs.internal.get(member, {}).items()):
            link = "{}/node_modules/{}".format(member, name)
            cmd.add("link", link, "../" * link.count("/") + package_paths[other])
        for name, dep in sorted(_deps_of(ctx, member).items()):
            info = dep[NpmPackageInfo]
            link = "{}/node_modules/{}".format(member, name)
            cmd.add("link", link, _link_target(out, link, info.package_dir))
            children.append(info.closure)
    ctx.actions.run(cmd, category = "npm_component", identifier = ctx.label.name)
    closure = ctx.actions.tset(NpmClosureTSet, value = out, children = children)
    return [
        DefaultInfo(default_output = out),
        NpmComponentInfo(
            members = {member: out.project(path) for member, path in package_paths.items()},
            closure = closure,
        ),
    ]

npm_component = rule(
    impl = _npm_component_impl,
    attrs = {
        "members": attrs.dict(
            attrs.string(),
            attrs.string(),
            doc = "Each member instance's npm name, by its name in the cell",
        ),
        "files": attrs.dict(attrs.string(), attrs.dep(), doc = "Each member's package contents"),
        "internal": attrs.dict(
            attrs.string(),
            attrs.dict(attrs.string(), attrs.string()),
            default = {},
            doc = "Each member's dependencies on other members, by import name",
        ),
        "deps": attrs.dict(
            attrs.string(),
            attrs.dict(attrs.string(), attrs.dep(providers = [NpmPackageInfo])),
            default = {},
            doc = "Each member's dependencies outside the cycle, by import name",
        ),
        "optional_deps": attrs.dict(
            attrs.string(),
            attrs.dict(attrs.string(), attrs.dep(providers = [NpmPackageInfo])),
            default = {},
            doc = "Each member's optional dependencies npm installs on the platform, likewise",
        ),
    },
    doc = "A dependency cycle's instances, laid out together in one output.",
)

def _npm_member_impl(ctx: AnalysisContext) -> list[Provider]:
    component = ctx.attrs.component[NpmComponentInfo]
    return [
        DefaultInfo(),
        NpmPackageInfo(package_dir = component.members[ctx.attrs.member], closure = component.closure),
    ]

npm_member = rule(
    impl = _npm_member_impl,
    attrs = {
        "component": attrs.dep(providers = [NpmComponentInfo]),
        "member": attrs.string(doc = "The member's name in the component"),
    },
    doc = "One instance of a dependency cycle, forwarding to its component.",
)

def npm_node_modules(ctx: AnalysisContext, npm_deps: list[Dependency]) -> (Artifact, NpmClosureTSet):
    """A consumer's node_modules: each of npm_deps (the jsdeps cell's direct
    dependencies, named by their npm name) linked to its package directory,
    and the closure every link reaches into, which must be a hidden input
    wherever node_modules is read."""
    packages = {}
    children = []
    for dep in npm_deps:
        info = dep[NpmPackageInfo]
        packages[dep.label.name] = info.package_dir
        children.append(info.closure)
    node_modules = ctx.actions.symlinked_dir("node_modules", packages)
    return node_modules, ctx.actions.tset(NpmClosureTSet, children = children)
