// A page whose browser allows the camera and the location when asked, each
// answer coming later, as a browser's does. Its camera takes a four-byte
// JPEG through ImageCapture, and its position is the one the Linux portal
// reports. The page counts the camera requests and the camera tracks still
// running when the program ends.
let cameraAsks = 0;
let running = 0;
const later = (value) => new Promise((resolve) => setTimeout(() => resolve(value), 10));
Object.defineProperty(globalThis, "navigator", {
  configurable: true,
  value: {
    mediaDevices: {
      getUserMedia: () => {
        cameraAsks += 1;
        running += 1;
        const track = {
          stop() {
            running -= 1;
          },
        };
        return later({ getTracks: () => [track], getVideoTracks: () => [track] });
      },
    },
    geolocation: {
      getCurrentPosition: (found) => setTimeout(() => found({ coords: { latitude: 52.52, longitude: 13.405, accuracy: 20 } }), 10),
    },
  },
});
globalThis.ImageCapture = class {
  takePhoto() {
    return later(new Blob([new Uint8Array([255, 216, 255, 217])], { type: "image/jpeg" }));
  }
};
globalThis.document = { addEventListener: () => {} };
const { jet_main } = await import("./app.js");
await jet_main();
process.stdout.write("camera asks: " + cameraAsks + ", cameras running: " + running + "\n");
