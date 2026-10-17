
process.on("unhandledRejection", (error) => {
  if (error?.name !== "JetError") throw error;
});
const { jet_main } = await import("./app.js");
try {
  await jet_main();
  throw new Error("unexpected success");
} catch (error) {
  if (error?.name !== "JetError") throw error;
  console.log("code=" + error.code);
  console.log(error.frame);
}
