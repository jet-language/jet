# 50000 sources no entry reaches.
mkdir -p tests/bulk
i=0
while [ $i -lt 50000 ]; do
    printf "v%d: fn() -> Int { %d }\n" $i $i > tests/bulk/f$i.jet
    i=$((i + 1))
done
