// A page on a device that moves and turns. Its browser allows the location,
// then reports two places as the device moves; it tilts the device upright,
// a little more upright, then onto its side; and its compass turns east,
// then south. Each sensor reports only while someone listens, as a
// browser's does, and the page counts the listeners and position watches
// still running when the program ends.
const listeners = new Map();
let watching = 0;
const after = (steps) => {
  let wait = 0;
  for (const step of steps) setTimeout(step, (wait += 10));
};
Object.defineProperty(globalThis, "navigator", {
  configurable: true,
  value: {
    geolocation: {
      watchPosition: (found) => {
        watching += 1;
        after([
          () => found({ coords: { latitude: 52.52, longitude: 13.405, accuracy: 20 } }),
          () => found({ coords: { latitude: 52.5201, longitude: 13.4051, accuracy: 10 } }),
        ]);
        return 7;
      },
      clearWatch: (id) => {
        if (id === 7) watching -= 1;
      },
    },
  },
});
const fire = (type, event) => {
  for (const listen of listeners.get(type) ?? []) listen(event);
};
globalThis.ondeviceorientationabsolute = null;
globalThis.addEventListener = (type, listen) => {
  listeners.set(type, [...(listeners.get(type) ?? []), listen]);
  if (type === "deviceorientation") {
    after([
      () => fire(type, { alpha: 0, beta: 90, gamma: 0, absolute: false }),
      () => fire(type, { alpha: 0, beta: 85, gamma: 2, absolute: false }),
      () => fire(type, { alpha: 0, beta: 0, gamma: 90, absolute: false }),
    ]);
  }
  if (type === "deviceorientationabsolute") {
    after([
      () => fire(type, { alpha: 270, beta: 90, gamma: 0, absolute: true }),
      () => fire(type, { alpha: 179.5, beta: 90, gamma: 0, absolute: true }),
    ]);
  }
};
globalThis.removeEventListener = (type, listen) => {
  listeners.set(
    type,
    (listeners.get(type) ?? []).filter((held) => held !== listen),
  );
};
globalThis.document = { addEventListener: () => {} };
const { jet_main } = await import("./app.js");
await jet_main();
const listening = ["deviceorientation", "deviceorientationabsolute"].reduce((count, type) => count + (listeners.get(type) ?? []).length, 0);
process.stdout.write("listening: " + listening + ", watching places: " + watching + "\n");
