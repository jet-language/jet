// A page whose browser answers as a person would: it allows the camera when
// asked, its site settings already refuse the location, and it allows
// notifications when asked. Each answer comes later, as a browser's does.
// The page counts the camera requests: a process asks once.
let cameraAsks = 0;
const later = (value) => new Promise((resolve) => setTimeout(() => resolve(value), 10));
Object.defineProperty(globalThis, "navigator", {
  configurable: true,
  value: {
    permissions: { query: ({ name }) => later({ state: name === "geolocation" ? "denied" : "prompt" }) },
    mediaDevices: {
      getUserMedia: () => {
        cameraAsks += 1;
        return later({ getTracks: () => [{ stop() {} }] });
      },
    },
    geolocation: { getCurrentPosition: (_, refuse) => setTimeout(() => refuse({ code: 1 }), 10) },
  },
});
globalThis.Notification = { permission: "default", requestPermission: () => later("granted") };
globalThis.document = { addEventListener: () => {} };
const { jet_main } = await import("./app.js");
await jet_main();
process.stdout.write("camera asks: " + cameraAsks + "\n");
