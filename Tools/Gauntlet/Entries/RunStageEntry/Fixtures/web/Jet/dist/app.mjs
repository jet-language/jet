// The built web artifact: its message and its module.
export const message = 'hello web';
export const module = new URL('./app.wasm', import.meta.url);
