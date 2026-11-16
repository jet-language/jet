files=0
bytes=0
while read -r size; do
  files=$((files + 1))
  bytes=$((bytes + size))
done < <(find w02 -type f -printf '%s\n')
echo "files=$files bytes=$bytes"
