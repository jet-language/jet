const { jet_main } = await import("./app.js");
try {
  await jet_main();
  console.log("unexpected success");
} catch (error) {
  console.log("status=" + error.status);
}
