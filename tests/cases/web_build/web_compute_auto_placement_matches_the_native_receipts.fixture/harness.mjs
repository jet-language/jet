import { readFileSync } from "node:fs";
import vm from "node:vm";

// Loads the live browser compute Prelude into a fresh realm whose only host
// global is `navigator`, so each run sees one browser GPU situation.
const source = readFileSync(new URL("./compute_webgpu.js", import.meta.url), "utf8");

function host(navigator) {
  const realm = vm.createContext(navigator === undefined ? {} : { navigator });
  vm.runInContext(source, realm);
  return realm;
}

function stub_device() {
  return {
    lost: new Promise(() => {}),
    queue: { writeBuffer() {}, async onSubmittedWorkDone() {} },
    createBuffer({ size }) {
      const bytes = new ArrayBuffer(size);
      return { getMappedRange: () => bytes, unmap() {} };
    },
  };
}

function unwrap(result) {
  if (result.tag !== "Ok") throw new Error(`expected Ok, got ${JSON.stringify(result)}`);
  return result.values[0];
}

async function report(label, realm) {
  const call = realm.jet_compute_web_call;
  const auto = await call("device_auto", []);
  console.log(`${label} auto:${realm.jet_compute_web_device_show(auto.kind)}`);
  const ones = unwrap(await call("ones", [[2n, 2n]]));
  console.log(`${label} device:${await call("device", [ones])}`);
  console.log(`${label} placement:${await call("placement", [ones])}`);
  const placed = unwrap(await call("on_device", [ones, auto]));
  console.log(`${label} auto-f64:${await call("placement", [placed])}`);
  const explicit = unwrap(await call("on_device", [ones, await call("device_cpu", [])]));
  console.log(`${label} explicit-cpu:${await call("placement", [explicit])}`);
  const f32 = realm.jet_compute_web_tensor([2], [1, 2], "cpu", "F32Strict+Reproducible", "harness=f32-source");
  const f32_placed = unwrap(await call("on_device", [f32, auto]));
  console.log(`${label} auto-f32:${await call("placement", [f32_placed])}`);
  const stream = unwrap(await call("stream_new_on", [auto]));
  console.log(`${label} stream:${await call("stream_show", [stream])}`);
}

await report("absent", host(undefined));
await report("no-adapter", host({ gpu: { async requestAdapter() { return null; } } }));
await report("gpu", host({ gpu: { async requestAdapter() { return { async requestDevice() { return stub_device(); } }; } } }));
