set -eu
# rs:1535-1542: overwrite the committed closure transaction, keeping its bytes for step-4.sh.
txn=$(ls -d jetpack/hangar/closure-db/journal/*.txn | awk 'NR == 1')
test -n "$txn"
chmod u+w "$txn"
cp "$txn" journal.original
printf '%s\n' "$txn" > journal.path
printf 'corrupt closure journal' > "$txn"
