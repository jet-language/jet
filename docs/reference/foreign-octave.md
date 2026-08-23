# Octave sidecar

Jet can call one-input, one-output matrix functions from an Octave `.m` file.
This is the `octave.*` binder from `D-FFI-OCTAVE1`.

## Bind a script

Write a function with one matrix input and one matrix output:

```octave
function result = scale(input)
  result = input * 2;
end
```

Generate the checked binding cache:

```text
jet inspect bind octave scale.m --pkg scale
```

The command writes `.jet/bindings/octave/scale.jet`, the static archive, and
the binding provenance file. The command needs `octave-cli` or `octave` and a
POSIX process supervisor.

## Call the binding

The generated module exposes a `Tensor` call. The adapter accepts rank-two
tensors and returns a rank-two tensor.

```jet
use octave.scale as scale
use core.compute as compute

fn run() -[FFI.Octave, IO]> {
    session :: scale.open() ?? panic("Octave sidecar did not start")
    input :: compute.matrix(2, 2, 3.0) ?? panic("matrix")
    output :: scale.scale(session, input, 5000) ?? panic("Octave call failed")
    print(compute.shape(output))
    print(compute.to_list(output))
    scale.close(^session)
}
```

The worker sends shape and data as JSON. It uses column-major order, which
matches Octave and the Jet `Tensor` wire contract. Jet checks the rank and the
element count before it constructs the result tensor.

The generated API reports `OctaveError` for a missing worker, timeout,
cancellation, protocol failure, command failure, shape mismatch, width
mismatch, or message-limit failure. The binder rejects multiple outputs,
missing arguments, invalid identifiers, and duplicate functions. It does not
translate unsupported Octave syntax or claim semantic equivalence.
