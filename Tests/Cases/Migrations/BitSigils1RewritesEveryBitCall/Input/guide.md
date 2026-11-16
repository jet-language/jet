# Masks

Clear a flag with `flags.bit_and(mask.bit_not())`; set one with `flags.bit_or(mask)`.

```jet
toggled: flags.bit_xor(mask)
flags = flags.bit_or(mask)
```

```sh
echo value.bit_and(1)
```
