// Internal C ABI atoms follow the existing D-ACRO-CASE1 acronym boundaries.
// Nominal/runtime type identities are never changed by this projection.
export function runtimeAbiAtom(name) {
  if (!/^[A-Za-z0-9_.]+$/.test(name)) throw new Error(`invalid runtime ABI atom ${name}`);
  return name.split(/[_.]/).filter(Boolean).map(segment => segment
    .replace(/([A-Z]+)([A-Z][a-z])/g, '$1_$2')
    .replace(/([a-z0-9])([A-Z])/g, '$1_$2').toLowerCase()).join('_');
}

// The generator owns both projections; Lower consumes this emitted helper.
export function emitRuntimeAbiAtom() {
  return `// Generated runtime ABI atom projection; nominal identities stay unchanged.
fn runtime_abi_atom(name: String) -> String Never! {
    bytes :: name.bytes()
    out := [U8]{}
    loop index in 0..<bytes.len() {
        code :: Int.from_u8(bytes[index])
        if code == 46 || code == 95 {
            if out.len() > 0 && out[out.len() - 1] != (U8{95}) -> &out.push(U8{95})
            next
        }
        upper :: code >= 65 && code <= 90
        if upper && index > 0 && out.len() > 0 && out[out.len() - 1] != (U8{95}) {
            previous :: Int.from_u8(bytes[index - 1])
            next_code :: if index + 1 < bytes.len() -> Int.from_u8(bytes[index + 1]) else -> 0
            previous_upper :: previous >= 65 && previous <= 90
            next_lower :: next_code >= 97 && next_code <= 122
            if !previous_upper || next_lower -> &out.push(U8{95})
        }
        &out.push(U8{if upper -> code + 32 else -> code})
    }
    if out.len() > 0 && out[out.len() - 1] == (U8{95}) {
        _ :: &out.pop() ?? panic("a nonempty runtime ABI atom failed to pop")
    }
    String.from_bytes(out) ?? panic("runtime ABI atom is not ASCII")
}
`;
}
