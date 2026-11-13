# A stand-in enum codec adapter for the enum-codec-bench entry, run from the
# repository root as `sh .../adapter.sh` with --name value pairs (appended by
# the harness or spelled out by a template). What it does depends on the form:
#   external   encodes and decodes correctly and writes full metrics, its
#              codec_ns depending on the sample index;
#   internal   writes the wrong wire, exits 3 on the known payload, accepts
#              the future variant, and writes metrics that are not JSON or
#              not an object;
#   untagged   writes no metrics for the known payload and metrics with a
#              negative or Boolean field for the others.
# ADAPTER_SLEEP=form-case-operation makes that cell sleep past any timeout.
operation= form= case= input= expected= output= metrics=
while [ $# -gt 1 ]; do
    case $1 in
        --operation) operation=$2 ;;
        --form) form=$2 ;;
        --case) case=$2 ;;
        --input) input=$2 ;;
        --expected) expected=$2 ;;
        --output) output=$2 ;;
        --metrics) metrics=$2 ;;
    esac
    shift 2
done
cell=$form-$case-$operation
stem=${output##*/}
stem=${stem%.output}
index=${stem##*-}
[ "$ADAPTER_SLEEP" = "$cell" ] && exec sleep 5
[ -f "$input" ] || { echo "missing input $input" >&2; exit 9; }
[ "$JET_ENUM_CODEC_BENCH_PROTOCOL" = 1 ] || { echo "no protocol" >&2; exit 9; }
case $form in
    external)
        printf '{"codec_ns": %d, "allocation_count": 3}' $((100 + index * 11)) > "$metrics"
        echo "external $cell" >&2
        if [ "$operation" = encode ]; then
            cat "$expected" > "$output"
            echo encoded
            exit 0
        fi
        [ "$case" = payload ] || exit 1
        echo decoded
        ;;
    internal)
        case $cell in
            internal-payload-encode) printf 'wrong' > "$output" ;;
            internal-payload-decode) exit 3 ;;
            internal-unknown-decode) printf 'nope' > "$metrics"; exit 1 ;;
            internal-malformed-decode) printf '[1]' > "$metrics"; exit 1 ;;
        esac
        ;;
    untagged)
        case $cell in
            untagged-payload-encode) cat "$expected" > "$output" ;;
            untagged-payload-decode) ;;
            untagged-malformed-decode) printf '{"codec_ns": -1}' > "$metrics"; exit 2 ;;
            untagged-evolution-decode) printf '{"allocation_count": true}' > "$metrics"; exit 2 ;;
            *) echo "untagged rejects $case" >&2; exit 2 ;;
        esac
        ;;
esac
exit 0
