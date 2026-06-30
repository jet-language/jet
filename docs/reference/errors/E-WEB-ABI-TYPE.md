# E-WEB-ABI-TYPE: `Point` cannot cross the JS/WASM boundary on a `#WasmExport` parameter

**Code:** `E-WEB-ABI-TYPE`

## What

`Point` cannot cross the JS/WASM boundary on a `#WasmExport` parameter

## Why

web exports and imports only admit ABI-safe types (scalars, `String`, `List`/`Map` of ABI-safe values, and `#[Codable]` structs/enums per D-JSBIND1)

## Fix

use a scalar, `String`, a `List`/`Map` of ABI-safe values, or a `#[Codable]` struct/enum whose fields are ABI-safe (D-JSBIND1)

## Example

Failing program: [`tests/ui/web_abi_type.jet`](../../tests/ui/web_abi_type.jet)

---

[Back to diagnostics registry](../admin/04-diagnostics.md)
