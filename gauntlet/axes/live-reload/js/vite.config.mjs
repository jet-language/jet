import { readFile } from "node:fs/promises";
import path from "node:path";

let version = 1;

export default {
  plugins: [{
    name: "jet-gauntlet-readiness",
    configureServer(server) {
      server.watcher.on("change", (file) => {
        if (path.basename(file) === "client.mjs") version += 1;
      });
      server.middlewares.use(async (request, response, next) => {
        const pathname = new URL(request.url ?? "/", "http://127.0.0.1").pathname;
        if (pathname === "/__axis_output") {
          try {
            response.statusCode = 200;
            response.setHeader("content-type", "text/plain; charset=utf-8");
            response.end(await readFile(path.resolve(process.cwd(), "client.mjs"), "utf8"));
          } catch (error) {
            response.statusCode = 500;
            response.end(`output unavailable: ${error.message}`);
          }
          return;
        }
        if (pathname !== "/__axis_ready") {
          next();
          return;
        }
        response.statusCode = 200;
        response.setHeader("content-type", "text/plain; charset=utf-8");
        response.end(String(version));
      });
    },
  }],
};
