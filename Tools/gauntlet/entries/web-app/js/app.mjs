export function mount(root = document.getElementById("app")) {
  let count = 0;
  const display = document.createElement("div");
  const increment = document.createElement("button");
  const reset = document.createElement("button");
  increment.textContent = "increment";
  reset.textContent = "reset";

  const render = () => { display.textContent = `count: ${count}`; };
  increment.addEventListener("click", () => { count += 1; render(); });
  reset.addEventListener("click", () => { count = 0; render(); });
  render();
  root.appendChild(display);
  root.appendChild(increment);
  root.appendChild(reset);
  return { display, increment, reset };
}
