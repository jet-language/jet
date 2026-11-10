#!/usr/bin/env jet
// A stand-in for integrate10.sh: prints the batch and SUBs it was given and
// exits with the code in rc/BATCH (0 when there is none).
use {
    files: core.files
    process: core.process
}

run: fn() {
    args: process.args()
    print("integrate10: {args.join(" ")}")
    code: (files.read("rc/{args[0]}") ?? "0").trim().to_int() ?? 0
    process.exit(code)
}
