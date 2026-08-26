# Jet Learn

`jet learn` runs the versioned first kata arc offline. It copies each kata to
`.jet/learn/first-arc/`, watches the current file with the same watch engine as
`jet dev`, and prints the live diagnostic followed by its `jet explain` lesson.

Run it from any directory:

```text
jet learn
```

Edit the path printed for the current kata. `jet learn --watch=off` checks one
iteration. `jet learn --check` validates every broken and solved source in the
toolchain's curriculum.
