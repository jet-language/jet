# Masks

Clear a flag with `flags.bit_and(mask.bit_not())`; set one with `flags.bit_or(mask)`.

```jet
toggled: flags.bit_xor(mask)
flags = flags.bit_or(mask)
ready: done && flags.bit_and(mask)
held: flags.bit_and(mask) && done
any: done || flags.bit_or(mask)
seen: flags.bit_or(mask) || done
clear: !flags.bit_and(mask)
```

```sh
echo value.bit_and(1)
```
