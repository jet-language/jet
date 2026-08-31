import net from "node:net";

const port = Number(process.argv[2] ?? 18080);
const pending = [];
const waiters = [];
let stopping = false;

function enqueue(job) {
  const resolve = waiters.shift();
  if (resolve) resolve(job);
  else pending.push(job);
}

function take() {
  if (pending.length > 0) return Promise.resolve(pending.shift());
  return new Promise((resolve) => waiters.push(resolve));
}

async function worker() {
  while (true) {
    const job = await take();
    if (job === null) return;
    await new Promise((resolve) => setTimeout(resolve, 1));
    job.resolve(job.value * job.value);
  }
}

const workers = Array.from({ length: 4 }, () => worker());

async function batch(count) {
  const values = Array.from({ length: count }, (_, index) => new Promise((resolve) => enqueue({ value: index + 1, resolve })));
  return (await Promise.all(values)).reduce((sum, value) => sum + value, 0);
}

function closeWorkers() {
  if (stopping) return;
  stopping = true;
  for (let index = 0; index < workers.length; index += 1) enqueue(null);
}

const server = net.createServer((socket) => {
  socket.setEncoding("utf8");
  let input = "";
  let handled = false;
  const reply = (message) => socket.end(`${message}\n`);
  socket.on("data", (chunk) => {
    input += chunk;
    if (handled || !input.includes("\n")) return;
    handled = true;
    const command = input.slice(0, input.indexOf("\n")).trim();
    if (command === "ready") {
      reply("ready");
      return;
    }
    const match = /^batch (\d+)$/.exec(command);
    if (match) {
      const count = Number(match[1]);
      if (count > 0 && count <= 32) {
        batch(count).then((total) => reply(`batch ${count} total ${total}`), () => reply("error"));
      } else {
        reply("error");
      }
      return;
    }
    if (command === "shutdown") {
      reply("bye");
      closeWorkers();
      server.close();
      return;
    }
    reply("error");
  });
});

server.listen(port, "127.0.0.1");
