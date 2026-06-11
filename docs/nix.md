# Nix / NixOS

The repo ships a [flake](https://nixos.wiki/wiki/Flakes) that builds the
`jet` CLI and wraps it so `rustc` (and a C linker) are on `PATH` when you
run `jet build` or `jet run`.

## Install on NixOS (flake input)

Add to your flake inputs (adjust the URL to your fork):

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    lex-lang.url = "github:YOUR_USER/lex-lang";
  };

  outputs = { nixpkgs, lex-lang, ... }: {
    nixosConfigurations.hostname = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ({ pkgs, ... }: {
          environment.systemPackages = [
            lex-lang.packages.${pkgs.system}.default
          ];
        })
      ];
    };
  };
}
```

Then `sudo nixos-rebuild switch` (or `home-manager` equivalent with
`home.packages`).

## Install on NixOS (local checkout)

```nix
environment.systemPackages = [
  (import /path/to/lex-lang { }).packages.${pkgs.system}.default
];
```

Or from the repo directory:

```bash
nix build
./result/bin/jet run examples/01_hello.jet
```

## Development

```bash
nix develop          # cargo, rustc, gcc, jet on PATH
cargo test
jet run examples/01_hello.jet
```

Legacy: `nix-shell` uses the same dev shell via `shell.nix`.

## Notes

- The compiler has **no** runtime dependency on Cargo for user programs —
  only `rustc` is invoked (see docs/03-architecture.md).
- FFI (M7) will need Cargo available when calling external Rust crates;
  add `cargo` to the wrap when that milestone lands.
