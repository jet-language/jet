
! command -v nix
cargo --version
rustc --version
cc --version
node --version
python3 --version
bwrap --version
printf '#include <stdio.h>\nint main(void) { puts("native-c-ok"); }\n' > "$TMPDIR/probe.c"
cc "$TMPDIR/probe.c" -o "$TMPDIR/probe"
test "$("$TMPDIR/probe")" = native-c-ok
printf 'JETPACK_NATIVE_TOOLS_OK\n'
