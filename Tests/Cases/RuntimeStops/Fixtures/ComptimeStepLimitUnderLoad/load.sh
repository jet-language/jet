# `sh load.sh`: keeps every core busy with a spinning shell, then checks
# under.jet and over.jet, saying each check's status and over.jet's report.
# The spinners are killed before the script ends.
spinners=
n=$(nproc)
while [ "$n" -gt 0 ]; do
    sh -c 'while :; do :; done' &
    spinners="$spinners $!"
    n=$((n - 1))
done
jet check under.jet > under.out 2>&1
echo "under: $?"
jet check over.jet > over.out 2>&1
echo "over: $?"
for p in $spinners; do
    kill -KILL "$p" 2>/dev/null || :
done
cat over.out
