# Octave sidecar

Jet can call one-input, one-output matrix functions from an Octave `.m` file.
This page is for numerical-code authors using the `octave.*` binder and for
reviewers checking the foreign-process boundary. The executable binder is
[`Source/CmdDevTools.rs`](../../../Source/CmdDevTools.rs), with input rules and
artifact generation in
[`crates/jet-pkg-model/src/OctaveBind.rs`](../../../crates/jet-pkg-model/src/OctaveBind.rs).
The boundary is identified by `D-FFI-OCTAVE1=A`.

## Bind a script

The source must define a top-level function with exactly one matrix input and
one matrix output:

```octave
function result = scale(input)
  result = input * 2;
end
```

Generate the checked binding cache with the explicit binder command:

```sh
jet inspect bind octave scale.m --pkg scale
```

The accepted usage is:

```text
jet inspect bind octave <script.m> [--pkg <lib>] [-o <out.jet>]
```

Without `-o`, the binder writes the package's default binding module under
`.jet/bindings/octave/`; with `-o`, it writes the requested Jet source path. It
also writes a generated C archive and `<lib>.provenance` beside the binding.
The binder checks the provisioned `octave-cli` (or `octave`), `cc`, and `ar`
tools and refuses a missing or failing tool rather than evaluating an
unvalidated script.

The parser rejects multiple outputs, missing arguments, duplicate functions,
invalid identifiers, and unsupported declarations. It parses the function
shape; it does not translate arbitrary Octave syntax or claim semantic
equivalence between the languages.

## Call the binding

The generated module supplies a supervised session API and one typed call per
bound function. Calls carry a rank-two `Tensor` and return a rank-two `Tensor`;
`OctaveError` covers a missing worker, timeout, cancellation, protocol or
command failure, shape/width mismatch, and message-limit failure.

A host program can use the generated module in the normal effect-declared loop:

```jet
use octave.scale as scale
use core.compute as compute

fn run() -[FFI.Octave, GPU, IO]> {
    session :: scale.open() ?? panic("Octave sidecar did not start")
    input :: compute.matrix(2, 2, 3.0) ?? panic("matrix")
    output :: scale.scale(session, input, 5000) ?? panic("Octave call failed")
    print(compute.shape(output))
    print(compute.to_list(output))
    scale.close(^session)
}
```

The generated API's `open`, function calls, `cancel`, and `close` operations
are all supervised foreign effects. Keep the session alive for the calls and
close it when ownership ends; a failed worker is represented as `OctaveError`,
not as an unchecked foreign exception.

## Wire contract and provenance

The worker sends shape and data through a bounded JSON protocol in column-major
order, matching Octave and the Jet `Tensor` wire contract. Jet checks rank and
element count before constructing the result tensor. Callers therefore declare
`GPU` for Tensor marshalling in addition to the Octave and process effects used
by the session.

The provenance record identifies the `jet-octave-bind-v1` schema, the exact
script and worker, the selected Octave tool, the archive tool identities,
`transport=json`, `order=column-major`, `shape=rank-2`, and the bounded session
limit. The archive and provenance are build inputs: editing the `.m` source or
changing the provisioned tool identity requires rebinding.
