# E-WEB-CROSS-PARTITION: `wasm_caller` is compiled to Wasm but calls `js_helper`, which lives in Js

**Code:** `E-WEB-CROSS-PARTITION`

## What

`wasm_caller` is compiled to Wasm but calls `js_helper`, which lives in Js

## Why

the web backend keeps DOM/view code in JS and compute in WASM; a direct call across that boundary is not allowed yet

## Fix

move the call behind a generated bridge, colocate both functions in the same bucket, or adjust `#Target` / `#Wasm` markers

## Example

Failing program: [`tests/ui/web_cross_partition.jet`](../../tests/ui/web_cross_partition.jet)

---

[Back to diagnostics registry](../admin/04-diagnostics.md)
