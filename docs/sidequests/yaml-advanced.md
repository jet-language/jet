# Full YAML 1.2 Advanced Features

**Card:** c16 / c153. **Decision:** D-ENC-YAML1 created this follow-up. **Status:** ready to build.

## Goal

Extend the shipped YAML core parser beyond serialized-config basics to cover
explicit tags and the remaining YAML 1.2 features needed for real Kubernetes,
CI, and infrastructure files.

## Scope

- explicit core tags such as `!!str`, `!!int`, `!!bool`;
- custom tags such as `!Ref` and `!Sub`;
- merge keys where they appear in common config;
- multi-document streams with document-level tag metadata;
- precise diagnostics for unsupported or malformed tag use.

## Build Plan

1. Extend the existing YAML token/parser in the emitted std prelude rather than
   creating a second parser.
2. Represent tags in the existing data tree so `yaml.parse` preserves them and
   typed `yaml.decode<T>` can either consume or reject them deliberately.
3. Add typed decode rules:
   - known core tags coerce through the existing scalar path;
   - unknown custom tags are preserved in dynamic parse results;
   - typed decode rejects unknown custom tags unless a codable field opts into
     tagged data through an already-ratified serde attribute.
4. Add diagnostics for malformed tag syntax and unsupported typed-decode tag use.
5. Add examples for Kubernetes-style YAML and CloudFormation-style custom tags.

## Verification

- YAML parser unit tests for tags, merge keys, and streams.
- Golden examples for dynamic parse/render and typed decode.
- `nix develop -c cargo test --test corelib`
- `nix develop -c cargo test`

