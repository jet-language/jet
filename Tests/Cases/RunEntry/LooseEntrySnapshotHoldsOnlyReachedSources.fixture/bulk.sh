# 50000 sources no entry reaches, and control/: the entry and the three
# sources it reaches, with nothing else beside them.
mkdir -p tests/bulk control/lib
cp tests/main.jet tests/util.jet control/
cp tests/lib/helper.jet tests/lib/inner.jet control/lib/
i=0
while [ $i -lt 50000 ]; do
    printf "v%d: fn() -> Int { %d }\n" $i $i > tests/bulk/f$i.jet
    i=$((i + 1))
done
