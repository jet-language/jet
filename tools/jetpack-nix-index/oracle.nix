# Off-device only. Import the exact nixexprs.tar.xz tree staged by the
# immutable channel release. No user overlays, registry, config, or flake input
# enters this evaluator.
{ nixexprs, system }:
let
  pkgs = import "${nixexprs}/default.nix" {
    inherit system;
    config = { };
    overlays = [ ];
  };
  packageInfo = import "${nixexprs}/pkgs/top-level/packages-info.nix" {
    inherit pkgs;
  };
  valid = value:
    builtins.isAttrs value
    && value ? pname
    && value ? version
    && value ? drvPath;
  outputInfo = value:
    builtins.mapAttrs
      (name: output: { inherit name; storePath = output; })
      (if value ? outputs then value.outputs else { out = value.outPath; });
  records = builtins.mapAttrs
    (attrpath: value: {
      # packageInfo is the inventory authority. Exact attrpath segments are
      # emitted by the caller instead of being recovered from Hydra job text.
      attrpath = [ attrpath ];
      version = value.version;
      drvPath = value.drvPath;
      outputs = outputInfo value;
      cache = false;
    })
    (builtins.filterAttrs (_: valid) packageInfo);
in
builtins.toJSON {
  inherit system;
  records = builtins.attrValues records;
}
