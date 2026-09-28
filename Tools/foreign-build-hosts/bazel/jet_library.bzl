"""Bazel macro for a checked Jet native Library output."""

def _package_root(path):
    slash = path.rfind("/")
    return path[:slash] if slash >= 0 else "."

def _relative_to_project(root, path):
    if root == ".":
        if path in [".", ".."] or path.startswith("../") or path.startswith("/"):
            fail("JET-HOST-INPUT: Bazel Jet input %s is outside the package project" % path)
        return path
    prefix = root + "/"
    if not path.startswith(prefix):
        fail("JET-HOST-INPUT: Bazel Jet input %s is outside the package project" % path)
    return path[len(prefix):]

def _jet_library_artifacts_impl(ctx):
    project = _package_root(ctx.file.manifest.short_path)
    entry = _relative_to_project(project, ctx.file.entry.short_path)
    outputs = [
        ctx.outputs.out_static,
        ctx.outputs.out_header,
        ctx.outputs.out_receipt,
        ctx.outputs.out_stamp,
    ]
    if ctx.outputs.out_jetlib:
        outputs.append(ctx.outputs.out_jetlib)

    args = ctx.actions.args()
    args.add("--jet", ctx.attr.jet)
    args.add("--project", project)
    args.add("--entry", entry)
    args.add("--output", ctx.attr.output)
    args.add("--library", ctx.attr.library)
    args.add("--dest", ctx.outputs.out_static.dirname)
    args.add("--kind", "static")
    args.add("--toolchain", ctx.attr.toolchain)
    args.add("--stage-project")
    args.add("--input", _relative_to_project(project, ctx.file.manifest.short_path))
    args.add("--input", _relative_to_project(project, ctx.file.lock.short_path))
    args.add("--input", entry)
    for dependency in ctx.files.deps:
        args.add("--input", _relative_to_project(project, dependency.short_path))
    if ctx.attr.loadable:
        args.add("--loadable")

    inputs = [ctx.file.manifest, ctx.file.lock, ctx.file.entry] + ctx.files.deps
    ctx.actions.run(
        executable = ctx.executable._runner,
        arguments = [args],
        inputs = depset(inputs),
        outputs = outputs,
        mnemonic = "JetLibrary",
        progress_message = "Exporting Jet Library %s" % ctx.attr.library,
        use_default_shell_env = True,
    )
    return [DefaultInfo(files = depset(outputs))]

_jet_library_artifacts = rule(
    implementation = _jet_library_artifacts_impl,
    attrs = {
        "entry": attr.label(allow_single_file = True, mandatory = True),
        "manifest": attr.label(allow_single_file = True, mandatory = True),
        "lock": attr.label(allow_single_file = True, mandatory = True),
        "deps": attr.label_list(allow_files = True),
        "output": attr.string(mandatory = True),
        "library": attr.string(mandatory = True),
        "jet": attr.string(mandatory = True),
        "toolchain": attr.string(mandatory = True),
        "loadable": attr.bool(default = False),
        "out_static": attr.output(mandatory = True),
        "out_header": attr.output(mandatory = True),
        "out_receipt": attr.output(mandatory = True),
        "out_stamp": attr.output(mandatory = True),
        "out_jetlib": attr.output(),
        "_runner": attr.label(
            default = Label("@jet_hosts//:jet-library"),
            executable = True,
            allow_single_file = True,
            cfg = "exec",
        ),
    },
)

def jet_library(
        name,
        entry,
        output,
        library,
        deps = [],
        kind = "static",
        loadable = False,
        jet = "jet",
        toolchain = "cc",
        linkopts = []):
    """Build one Jet Library and expose its archive/header as a cc_library.

    The action receives every input as a separate argv element. `deps` is the
    complete Jet source closure beyond entry, package.jet, and .jet/lock.
    """
    if kind != "static":
        fail("JET-HOST-ABI: Bazel jet_library currently exports the native static archive only")
    if not library.replace("_", "").replace("-", "").isalnum():
        fail("JET-HOST-ABI: library must be an alphanumeric, underscore, or hyphen name")

    jet_target = name + "_jet"
    static_name = "lib%s.a" % library
    header_name = "%s.h" % library
    out_static = jet_target + "/" + static_name
    out_header = jet_target + "/" + header_name
    rule_args = {
        "name": jet_target,
        "entry": entry,
        "manifest": "package.jet",
        "lock": ".jet/lock",
        "deps": deps,
        "output": output,
        "library": library,
        "jet": jet,
        "toolchain": toolchain,
        "loadable": loadable,
        "out_static": out_static,
        "out_header": out_header,
        "out_receipt": jet_target + "/jet-host.receipt",
        "out_stamp": jet_target + "/jet-host.stamp",
    }
    if loadable:
        rule_args["out_jetlib"] = jet_target + "/%s.jetlib" % library
    _jet_library_artifacts(**rule_args)
    native.cc_import(
        name = name + "_archive",
        static_library = ":" + out_static,
    )
    native.cc_library(
        name = name,
        hdrs = [":" + out_header],
        includes = [jet_target],
        deps = [":" + name + "_archive"],
        linkopts = linkopts,
    )
