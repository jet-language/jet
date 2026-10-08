# Match activation_hash in tests/env_hook.rs: strip the exact prefix and suffix.
found=0
hash=
while IFS= read -r line; do
    case "$line" in
        "export JETPACK_ENV_HASH='"*"'")
            hash=${line#*=}
            hash=${hash#\'}
            hash=${hash%\'}
            found=1
            break
            ;;
    esac
done < activation.sh
if [ "$found" != 1 ]; then
    printf '%s\n' 'activation must export its definition hash' >&2
    exit 1
fi
export JETPACK_ENV_DIR="$2"
export JETPACK_ENV_HASH="$hash"
exec "$1" env export bash
