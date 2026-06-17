# Sidequest: JetOS — D-OS4 priority map + D-OS6 user syntax

**Ratified:** 2026-06-17  
**Track:** jetpack/jetos (separate from epoch-2-impl)  
**Plan:** `docs/plans/jetpack-jetos/jetos-design.md`

## D-OS4 — Priority map

**Ratified option C:** priority uses a map with named keys, not positional priority numbers.

```jet
service sshd {
    priority = [default: 50, force: 100];
}
```

- `default` key: the base scheduling priority
- `force` key: override priority (analogous to `mkForce` in NixOS)

Update `jetos-design.md` to reflect this syntax. Register the syntax in `src/syntax.rs` under the jetos/jetpack section if the `priority` field type uses a special literal form. If it's just a record literal, no new syntax registration needed.

## D-OS6 — User syntax

**Ratified option A:** `user.<name>.*` namespace with `user.me` alias.

```jet
user.alice {
    shell = "fish";
    home_manager = { … };
}

// alias for the current user (owner of the config):
user.me {
    shell = "fish";
}
```

- `user.me` is a stable alias for whoever owns the jetos config being built
- Regular users are `user.<name>`
- No magic — `user.me` is just a well-known name, not a runtime concept

Update `jetos-design.md` with the ratified syntax examples. If `user.me` needs special parser treatment, note it in the design doc and flag it to the owner (I7 requires a syntax.rs entry if it's a reserved name in the jetos context).

## What to update

1. `docs/plans/jetpack-jetos/jetos-design.md` — add D-OS4 and D-OS6 ratified examples to the relevant sections
2. Confirm whether `user.me` needs a `src/syntax.rs` entry (reserved identifier in jetos context)

## Exit criteria

- `jetos-design.md` shows the ratified syntax for both priorities and users
- Any new reserved identifiers registered in `src/syntax.rs` (if applicable)
- `nix develop -c cargo test` still green
