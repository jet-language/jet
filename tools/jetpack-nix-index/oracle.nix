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
  outputPath = output:
    if builtins.isString output then output
    else if builtins.isAttrs output && output ? path then output.path
    else if builtins.isAttrs output && output ? outPath then output.outPath
    else throw "nix index oracle: output has no store path";
  outputInfo = value:
    builtins.attrValues (builtins.mapAttrs
      (name: output: { name = name; storePath = outputPath output; })
      (if value ? outputs then value.outputs else { out = value.outPath; }));
  collect = attrpath: value:
    if valid value then [ {
      inherit attrpath;
      version = value.version;
      drvPath = value.drvPath;
      outputs = outputInfo value;
      cache = false;
    } ] else if builtins.isAttrs value then
      builtins.concatLists (builtins.map
        (name: collect (attrpath ++ [ name ]) (builtins.getAttr name value))
        (builtins.attrNames value))
    else [ ];
  records = builtins.concatLists (builtins.map
    (name: collect [ name ] (builtins.getAttr name packageInfo))
    (builtins.attrNames packageInfo));
in
builtins.toJSON {
  inherit system;
  inherit records;
}
