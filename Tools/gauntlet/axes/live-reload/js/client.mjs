export const marker = "reload-before";

if (typeof document !== "undefined") {
  document.getElementById("axis-ready").textContent = marker;
}
