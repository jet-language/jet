// A page whose browser has no JavaScript Promise Integration: the host reads
// WebAssembly.Suspending and WebAssembly.promising when it loads, so they are
// gone before app.js is.
delete WebAssembly.Suspending;
delete WebAssembly.promising;
const { jet_main } = await import("./app.js");
await jet_main();
