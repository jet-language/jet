# E-WEB-TARGET-BROWSER: `bad` is pinned to Wasm but uses the `Browser` effect

**Code:** `E-WEB-TARGET-BROWSER`

## What

`bad` is pinned to Wasm but uses the `Browser` effect

## Why

the web backend keeps DOM/view code in JS and compute in WASM; a Wasm-pinned function cannot call browser APIs directly

## Fix

remove the `#Target(Wasm)` pin, move browser work into a `#Target(JS)` function, or drop the browser API calls

## Example

Failing program: [`tests/ui/web_target_browser.jet`](../../../tests/ui/web_target_browser.jet)

---

[Back to diagnostics registry](../../spec/diagnostics.md)
