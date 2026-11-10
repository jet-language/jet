#!/usr/bin/env jet
// A stand-in for integrate10.sh: prints the batch and SUBs it was given and
// what the seed it was handed in CENSUS_JET_DIR holds.
use {
    files: core.files
    process: core.process
}

run: fn() {
    seed: process.env_get("CENSUS_JET_DIR") ?? ""
    here: files.cwd() ?? ""
    print("integrate10: CENSUS_JET_DIR={seed} args={process.args().join(" ")}")
    names: [String]{"jet", "IDENTITY", "jet-runtime/lib.txt", "jet-runtime/sub/inner.txt"}
    loop name in names -> print("{name}: {(files.read("{seed}/{name}") ?? "missing").trim()}")
    executable := false
    if files.stat("{seed}/jet") == {
        .Ok(info) -> executable = info.mode % 128 >= 64
        .Err(_) -> ()
    }
    print("jet is executable: {executable}")
    print("jet-runtime is a link: {files.is_symlink("{seed}/jet-runtime")}")
    print("jetpack -> {(files.read_link("{seed}/jetpack") ?? "none").removeprefix("{here}/")}")
}
