# Build infrastructure helper. The caller supplies a clean nixpkgs import and
# runs this expression once per sorted batch of at most 256 oracle attrpaths.
{ pkgs, attrpaths, revision, system }:
let
  records = builtins.map
    (attrpath:
      let
        value = builtins.getAttrFromPath attrpath pkgs;
        outputPath = output:
          if builtins.isString output then output
          else if builtins.isAttrs output && output ? path then output.path
          else if builtins.isAttrs output && output ? outPath then output.outPath
          else throw "nix index differential: output has no store path";
        outputs = if value ? outputs then value.outputs else { out = value.outPath; };
      in {
        inherit attrpath;
        version = if value ? version then value.version else "";
        drvPath = value.drvPath;
        outputs = builtins.attrValues (builtins.mapAttrs
          (name: output: { name = name; storePath = outputPath output; })
          outputs);
      })
    attrpaths;
in {
  inherit revision system records;
}
