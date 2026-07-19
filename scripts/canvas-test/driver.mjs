import { spawn } from "node:child_process";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";

const MESSAGE_DELIMITER = "\0";

export class CdpDriver {
  constructor(options = {}) {
    this.chrome = options.chrome || process.env.CHROMIUM || "chromium";
    this.headless = options.headless !== false;
    this.chromeTempRoot = options.chromeTempRoot || (process.platform === "win32" ? tmpdir() : "/tmp");
    this.userDataDir = options.userDataDir || null;
    this.child = null;
    this.exited = null;
    this.failure = null;
    this.nextId = 1;
    this.pending = new Map();
    this.sessions = new Map();
    this.buffer = "";
    this.pageSession = null;
    this.stderr = "";
  }

  async launch() {
    this.userDataDir ||= await mkdtemp(join(this.chromeTempRoot, "jet-cdp-"));
    await mkdir(this.userDataDir, { recursive: true });
    const args = [
      this.headless ? "--headless=new" : "",
      "--remote-debugging-pipe",
      "--disable-gpu",
      "--disable-dev-shm-usage",
      "--no-first-run",
      "--no-default-browser-check",
      `--user-data-dir=${this.userDataDir}`,
      "about:blank",
    ].filter(Boolean);
    this.child = spawn(this.chrome, args, {
      stdio: ["ignore", "ignore", "pipe", "pipe", "pipe"],
      env: { ...process.env, TMPDIR: this.chromeTempRoot },
    });
    this.child.stderr.setEncoding("utf8");
    this.child.stderr.on("data", (chunk) => {
      this.stderr += chunk;
    });
    this.child.stdio[4].setEncoding("utf8");
    this.child.stdio[4].on("data", (chunk) => this.#read(chunk));
    for (const pipe of [this.child.stdio[3], this.child.stdio[4]]) {
      pipe.on("error", (error) => this.#fail(new Error(`chromium CDP pipe failed: ${error.message}\n${this.stderr}`)));
    }
    this.child.on("error", (error) => this.#fail(new Error(`could not start chromium: ${error.message}`)));
    this.exited = new Promise((resolve) => this.child.on("close", (code, signal) => {
      this.#fail(new Error(`chromium exited (${code ?? signal})\n${this.stderr}`));
      resolve({ code, signal });
    }));
    try {
      await this.send("Browser.getVersion");
      const target = await this.send("Target.createTarget", { url: "about:blank" });
      const attach = await this.send("Target.attachToTarget", {
        targetId: target.targetId,
        flatten: true,
      });
      this.pageSession = attach.sessionId;
      await this.send("Page.enable", {}, this.pageSession);
      await this.send("Runtime.enable", {}, this.pageSession);
      await this.send("DOM.enable", {}, this.pageSession);
      return this;
    } catch (error) {
      await this.close();
      throw error;
    }
  }

  async close() {
    try {
      if (this.child && this.child.exitCode === null && this.child.signalCode === null) this.child.kill("SIGKILL");
      if (this.exited) await this.exited;
    } finally {
      if (this.userDataDir) await rm(this.userDataDir, { recursive: true, force: true });
    }
  }

  send(method, params = {}, sessionId = undefined) {
    if (this.failure) return Promise.reject(this.failure);
    if (!this.child) return Promise.reject(new Error("chromium is not running"));
    const id = this.nextId++;
    const message = JSON.stringify({ id, method, params, sessionId });
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`CDP timeout: ${method}`));
      }, 15000);
      this.pending.set(id, { resolve, reject, timer, method });
      this.child.stdio[3].write(message + MESSAGE_DELIMITER, (error) => {
        if (!error || !this.pending.has(id)) return;
        this.#fail(new Error(`CDP write failed: ${method}: ${error.message}`));
      });
    });
  }

  #fail(error) {
    this.failure ||= error;
    for (const { reject, timer } of this.pending.values()) {
      clearTimeout(timer);
      reject(this.failure);
    }
    this.pending.clear();
    for (const listeners of this.sessions.values()) {
      for (const { reject, timer } of listeners) {
        clearTimeout(timer);
        reject(this.failure);
      }
    }
    this.sessions.clear();
  }

  async navigate(url) {
    const loaded = this.waitForEvent("Page.loadEventFired", this.pageSession);
    await this.send("Page.navigate", { url }, this.pageSession);
    await loaded;
  }

  async evaluate(expression, options = {}) {
    const result = await this.send(
      "Runtime.evaluate",
      {
        expression,
        awaitPromise: options.awaitPromise !== false,
        returnByValue: options.returnByValue !== false,
        userGesture: true,
      },
      this.pageSession,
    );
    if (result.exceptionDetails) {
      throw new Error(result.exceptionDetails.text || "Runtime.evaluate failed");
    }
    return result.result ? result.result.value : undefined;
  }

  async click(x, y, button = "left") {
    await this.send("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button, clickCount: 1 }, this.pageSession);
    await this.send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button, clickCount: 1 }, this.pageSession);
  }

  async rightClick(x, y) {
    await this.click(x, y, "right");
  }

  async drag(from, to, steps = 12) {
    await this.send("Input.dispatchMouseEvent", { type: "mousePressed", x: from.x, y: from.y, button: "left", clickCount: 1 }, this.pageSession);
    for (let i = 1; i <= steps; i++) {
      const t = i / steps;
      await this.send("Input.dispatchMouseEvent", {
        type: "mouseMoved",
        x: from.x + (to.x - from.x) * t,
        y: from.y + (to.y - from.y) * t,
        button: "left",
        buttons: 1,
      }, this.pageSession);
    }
    await this.send("Input.dispatchMouseEvent", { type: "mouseReleased", x: to.x, y: to.y, button: "left", clickCount: 1 }, this.pageSession);
  }

  async wheel(x, y, deltaY, deltaX = 0) {
    await this.send("Input.dispatchMouseEvent", { type: "mouseWheel", x, y, deltaX, deltaY }, this.pageSession);
  }

  async type(text) {
    for (const ch of text) {
      await this.send("Input.dispatchKeyEvent", { type: "char", text: ch, unmodifiedText: ch }, this.pageSession);
    }
  }

  async press(key) {
    await this.send("Input.dispatchKeyEvent", { type: "rawKeyDown", key }, this.pageSession);
    await this.send("Input.dispatchKeyEvent", { type: "keyUp", key }, this.pageSession);
  }

  async screenshot(path) {
    const result = await this.send("Page.captureScreenshot", { format: "png", fromSurface: true }, this.pageSession);
    const bytes = Buffer.from(result.data, "base64");
    await writeFile(path, bytes);
    return path;
  }

  waitForEvent(method, sessionId = undefined, timeoutMs = 15000) {
    return new Promise((resolve, reject) => {
      const key = sessionId ? `${sessionId}:${method}` : method;
      const timer = setTimeout(() => {
        const listeners = this.sessions.get(key) || [];
        this.sessions.set(key, listeners.filter((listener) => listener.resolve !== resolve));
        reject(new Error(`CDP event timeout: ${method}`));
      }, timeoutMs);
      const listeners = this.sessions.get(key) || [];
      listeners.push({ resolve, reject, timer });
      this.sessions.set(key, listeners);
    });
  }

  #read(chunk) {
    this.buffer += chunk;
    let idx;
    while ((idx = this.buffer.indexOf(MESSAGE_DELIMITER)) >= 0) {
      const raw = this.buffer.slice(0, idx);
      this.buffer = this.buffer.slice(idx + 1);
      if (!raw.trim()) continue;
      const msg = JSON.parse(raw);
      if (msg.id && this.pending.has(msg.id)) {
        const item = this.pending.get(msg.id);
        clearTimeout(item.timer);
        this.pending.delete(msg.id);
        if (msg.error) item.reject(new Error(`${item.method}: ${msg.error.message}`));
        else item.resolve(msg.result || {});
        continue;
      }
      if (msg.method) {
        const key = msg.sessionId ? `${msg.sessionId}:${msg.method}` : msg.method;
        const listeners = this.sessions.get(key) || [];
        this.sessions.delete(key);
        for (const listener of listeners) {
          clearTimeout(listener.timer);
          listener.resolve(msg.params || {});
        }
      }
    }
  }
}
