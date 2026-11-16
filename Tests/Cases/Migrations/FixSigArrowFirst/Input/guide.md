# Saving

A unit function that can fail and has effects writes its contract first:

```jet
save: fn(path: String) IOError! -[FS.Write]> {
    files.write(path, "done")
}
```

The callback type is `fn(String) IOError! -[FS.Write]>`, and a pure one is
`fn(String) IOError!`. A published API row reads
`fn save(path: String) IOError! -[FS.Write]>`.
