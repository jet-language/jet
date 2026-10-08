
process.on("unhandledRejection", (error) => {
  if (error?.name !== "JetError") throw error;
});
const { jet_main } = await import("./app.js");
try {
  await jet_main();
  console.log("unexpected success");
} catch (error) {
  console.log("kind=" + error.constructor?.name);
  console.log("code=" + error.code);
  console.log("journey=" + error.journey);
  console.log("frame=" + error.frame);
}
