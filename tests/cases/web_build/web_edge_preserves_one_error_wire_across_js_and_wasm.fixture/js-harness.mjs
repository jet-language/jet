
process.on("unhandledRejection", (error) => {
  if (error?.name !== "JetError") throw error;
});
const { jet_main } = await import("./app.js");
try {
  await jet_main();
  console.log("unexpected success");
} catch (error) {
  console.log("kind=" + (error.constructor?.name ?? ""));
  console.log("instance=" + (error instanceof Error));
  console.log("name=" + error.name);
  console.log("code=" + error.code);
  console.log("message=" + error.message);
  console.log("cause=" + (error.cause?.message ?? error.cause ?? ""));
  console.log("wire=" + JSON.stringify(error.toJSON()));
  console.log("journey=" + (error.journey ?? ""));
  console.log("frame=" + JSON.stringify(error.frame));
}
