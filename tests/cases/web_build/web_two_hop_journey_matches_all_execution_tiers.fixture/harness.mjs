
process.on("unhandledRejection", (error) => {
  if (error?.name !== "JetError") throw error;
});
const { jet_main } = await import("./app.js");
try {
  await jet_main();
  throw new Error("unexpected success");
} catch (error) {
  console.log("JOURNEY_BEGIN");
  console.log(error.journey);
  console.log("JOURNEY_END");
  console.log("FRAME_BEGIN");
  console.log(error.frame);
  console.log("FRAME_END");
}
