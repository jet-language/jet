#!/bin/sh
# Deterministic CLI double for staging, stream, environment and companion tests.
verb=$1
shift
verbose=0
if [ "${1:-}" = --verbose ]; then verbose=1; shift; fi
file=${1:-}
case "$verb" in
check)
    if [ ! -f "$file" ]; then echo 'fixture was not staged' >&2; exit 9; fi
    case "$file" in
    *extension*)
        if [ "${JET_COMPILER_EXTENSION##*/}" != hook.wasm ] || [ ! -f "$JET_COMPILER_EXTENSION" ]; then echo 'wrong extension' >&2; exit 9; fi
        ;;
    *)
        if [ -n "${JET_COMPILER_EXTENSION:-}" ]; then echo 'leaked extension' >&2; exit 9; fi
        ;;
    esac
    case "$file" in
    *published*)
        if [ ! -f "${JET_SCHEMA_CACHE_DIR:-}/Record.snapshot" ]; then echo 'missing published schema' >&2; exit 9; fi
        ;;
    *)
        if [ -n "${JET_SCHEMA_CACHE_DIR:-}" ]; then echo 'leaked schema cache' >&2; exit 9; fi
        ;;
    esac
    case "$file" in
    *project/run.jet)
        if [ ! -f "$(dirname "$file")/.jet/authored" ]; then echo 'fixture state not staged' >&2; exit 9; fi
        ;;
    esac
    case "$file" in
    *lint*)
        if [ "$verbose" != 1 ]; then echo 'lint requires verbose' >&2; exit 9; fi
        printf 'Warning [L0302]: lint\n\nok: `%s` has no errors (1 warning)\nrun `jet explain L0302` to learn more\n' "$file" >&2
        ;;
    *clean*|*published*|*extension*|*.fixed.jet) ;;
    *)
        printf 'Error [E0109]: mismatch\n\n1 problem found\nrun `jet explain E0109` to learn more\n' >&2
        exit 1
        ;;
    esac
    ;;
build)
    if [ ! -f "$file" ]; then echo 'fixed was not staged' >&2; exit 9; fi
    if [ "${2:-}" = --offline ]; then
        if [ "$file" != main.jet ] || [ ! -f package.jet ]; then echo 'unlocked fixture was not staged' >&2; exit 9; fi
        printf 'Error [E1250]: unlocked toolchain\n' >&2
        exit 1
    fi
    printf '#!/bin/sh\nexit 0\n' > fixture-artifact
    chmod +x fixture-artifact
    printf 'built: fixture-artifact\n'
    ;;
run)
    if [ ! -f "$file" ]; then echo 'fixed was not staged' >&2; exit 9; fi
    ;;
*) echo 'unexpected test invocation' >&2; exit 9 ;;
esac
