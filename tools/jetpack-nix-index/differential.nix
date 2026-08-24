# Build infrastructure helper. The caller supplies a clean nixpkgs import and
# runs this expression once per sorted batch of at most 256 oracle attrpaths.
{ pkgs, attrpaths }:
builtins.map
  (attrpath:
    let
      value = builtins.getAttrFromPath attrpath pkgs;
      outputs = if value ? outputs then value.outputs else { out = value.outPath; };
    in {
      inherit attrpath;
      version = if value ? version then value.version else "";
      drvPath = value.drvPath;
      outputs = builtins.mapAttrs
        (name: storePath: { inherit name; inherit storePath; })
        outputs;
    })
  attrpaths
