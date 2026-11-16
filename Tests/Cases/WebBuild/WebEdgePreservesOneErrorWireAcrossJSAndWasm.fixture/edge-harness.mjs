// Runs the web build's program. The runtime writes its report to stderr;
// the host passes the bytes through and only reports the exit status.
const { jet_main } = await import("./app.js");
try {
  await jet_main();
  console.log("unexpected success");
} catch (error) {
  console.log(`${error.name} status=${error.status}`);
}
