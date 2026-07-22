import { spawn } from "node:child_process";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:net";
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
    this.metadata = { browser: "chromium", version: "unknown" };
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
      const version = await this.send("Browser.getVersion");
      this.metadata.version = version.product || version.userAgent || "unknown";
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
    if (this.failure) throw this.failure;
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

  async shortcut(keys) {
    let modifiers = 0;
    const bits = { Alt: 1, Control: 2, Meta: 4, Shift: 8 };
    for (const key of keys) {
      modifiers |= bits[key] || 0;
      await this.send("Input.dispatchKeyEvent", { type: "rawKeyDown", key, modifiers }, this.pageSession);
    }
    for (const key of [...keys].reverse()) {
      await this.send("Input.dispatchKeyEvent", { type: "keyUp", key, modifiers }, this.pageSession);
      modifiers &= ~(bits[key] || 0);
    }
  }

  async screenshot(path) {
    const result = await this.send("Page.captureScreenshot", { format: "png", fromSurface: true }, this.pageSession);
    const bytes = Buffer.from(result.data, "base64");
    await writeFile(path, bytes);
    return path;
  }

  waitForEvent(method, sessionId = undefined, timeoutMs = 15000) {
    if (this.failure) return Promise.reject(this.failure);
    return new Promise((resolve, reject) => {
      const key = sessionId ? `${sessionId}:${method}` : method;
      const timer = setTimeout(() => {
        const listeners = this.sessions.get(key) || [];
        const remaining = listeners.filter((listener) => listener.resolve !== resolve);
        if (remaining.length) this.sessions.set(key, remaining);
        else this.sessions.delete(key);
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

const WEBDRIVER_KEYS = {
  Alt: "\uE00A",
  Control: "\uE009",
  Enter: "\uE007",
  Escape: "\uE00C",
  Meta: "\uE03D",
  Shift: "\uE008",
};

async function freePort() {
  return await new Promise((resolve, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close((error) => error ? reject(error) : resolve(port));
    });
  });
}

export class GeckoDriver {
  constructor(options = {}) {
    this.firefox = options.firefox || process.env.FIREFOX || "firefox";
    this.geckodriver = options.geckodriver || process.env.GECKODRIVER || "geckodriver";
    this.headless = options.headless !== false;
    this.tempRoot = options.tempRoot || (process.platform === "win32" ? tmpdir() : "/tmp");
    this.profileRoot = null;
    this.child = null;
    this.exited = null;
    this.failure = null;
    this.sessionId = null;
    this.baseUrl = null;
    this.stderr = "";
    this.metadata = { browser: "firefox", version: "unknown" };
    this.closing = false;
    this.processGroup = process.platform !== "win32";
  }

  async launch() {
    this.profileRoot = await mkdtemp(join(this.tempRoot, "jet-gecko-"));
    const port = await freePort();
    this.baseUrl = `http://127.0.0.1:${port}`;
    this.child = spawn(this.geckodriver, ["--host", "127.0.0.1", "--port", String(port)], {
      stdio: ["ignore", "ignore", "pipe"],
      env: { ...process.env, TMPDIR: this.profileRoot },
      detached: this.processGroup,
    });
    this.child.stderr.setEncoding("utf8");
    this.child.stderr.on("data", (chunk) => { this.stderr += chunk; });
    this.child.on("error", (error) => this.#fail(new Error(`could not start geckodriver: ${error.message}`)));
    this.exited = new Promise((resolve) => this.child.on("close", (code, signal) => {
      if (!this.closing) this.#fail(new Error(`geckodriver exited (${code ?? signal})\n${this.stderr}`));
      resolve({ code, signal });
    }));
    try {
      await this.#waitUntilReady();
      const value = await this.#request("POST", "/session", {
        capabilities: {
          alwaysMatch: {
            browserName: "firefox",
            "moz:firefoxOptions": {
              binary: this.firefox,
              args: this.headless ? ["-headless"] : [],
            },
          },
        },
      }, false);
      this.sessionId = value.sessionId;
      const capabilities = value.capabilities || {};
      this.metadata = {
        browser: capabilities.browserName || "firefox",
        version: capabilities.browserVersion || "unknown",
      };
      return this;
    } catch (error) {
      await this.close();
      throw error;
    }
  }

  async close() {
    this.closing = true;
    let cleanupError = null;
    try {
      if (this.sessionId) {
        try {
          await this.#request("DELETE", `/session/${this.sessionId}`, undefined, false);
        } catch (error) {
          cleanupError = error;
        }
      }
      this.sessionId = null;
      try {
        await this.#terminateProcessTree();
      } catch (error) {
        cleanupError ||= error;
      }
    } finally {
      if (this.profileRoot) await rm(this.profileRoot, { recursive: true, force: true });
    }
    if (cleanupError) throw cleanupError;
  }

  async navigate(url) {
    await this.#sessionRequest("POST", "/url", { url });
  }

  async evaluate(expression) {
    return await this.#sessionRequest("POST", "/execute/sync", {
      script: `return (${expression});`,
      args: [],
    });
  }

  async click(x, y, button = "left") {
    await this.#pointer([
      { type: "pointerMove", duration: 0, origin: "viewport", x: Math.round(x), y: Math.round(y) },
      { type: "pointerDown", button: button === "right" ? 2 : 0 },
      { type: "pointerUp", button: button === "right" ? 2 : 0 },
    ]);
  }

  async rightClick(x, y) {
    await this.click(x, y, "right");
  }

  async drag(from, to, steps = 12) {
    const actions = [
      { type: "pointerMove", duration: 0, origin: "viewport", x: Math.round(from.x), y: Math.round(from.y) },
      { type: "pointerDown", button: 0 },
    ];
    for (let i = 1; i <= steps; i++) {
      const t = i / steps;
      actions.push({
        type: "pointerMove",
        duration: 16,
        origin: "viewport",
        x: Math.round(from.x + (to.x - from.x) * t),
        y: Math.round(from.y + (to.y - from.y) * t),
      });
    }
    actions.push({ type: "pointerUp", button: 0 });
    await this.#pointer(actions);
  }

  async wheel(x, y, deltaY, deltaX = 0) {
    await this.#actions([{
      type: "wheel",
      id: "wheel",
      actions: [{
        type: "scroll",
        duration: 0,
        origin: "viewport",
        x: Math.round(x),
        y: Math.round(y),
        deltaX: Math.round(deltaX),
        deltaY: Math.round(deltaY),
      }],
    }]);
  }

  async type(text) {
    const actions = [];
    for (const ch of text) actions.push({ type: "keyDown", value: ch }, { type: "keyUp", value: ch });
    await this.#key(actions);
  }

  async press(key) {
    const value = WEBDRIVER_KEYS[key] || key;
    await this.#key([{ type: "keyDown", value }, { type: "keyUp", value }]);
  }

  async shortcut(keys) {
    const values = keys.map((key) => WEBDRIVER_KEYS[key] || key);
    await this.#key([
      ...values.map((value) => ({ type: "keyDown", value })),
      ...[...values].reverse().map((value) => ({ type: "keyUp", value })),
    ]);
  }

  async screenshot(path) {
    const data = await this.#sessionRequest("GET", "/screenshot");
    await writeFile(path, Buffer.from(data, "base64"));
    return path;
  }

  async #pointer(actions) {
    await this.#actions([{ type: "pointer", id: "mouse", parameters: { pointerType: "mouse" }, actions }]);
  }

  async #key(actions) {
    await this.#actions([{ type: "key", id: "keyboard", actions }]);
  }

  async #actions(actions) {
    await this.#sessionRequest("POST", "/actions", { actions });
    await this.#sessionRequest("DELETE", "/actions").catch(() => {});
  }

  async #sessionRequest(method, path, body = undefined) {
    if (!this.sessionId) throw new Error("firefox WebDriver session is not running");
    return await this.#request(method, `/session/${this.sessionId}${path}`, body);
  }

  async #request(method, path, body = undefined, includeStderr = true) {
    if (this.failure) throw this.failure;
    const response = await fetch(`${this.baseUrl}${path}`, {
      method,
      headers: body === undefined ? undefined : { "content-type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
      signal: AbortSignal.timeout(15000),
    });
    const payload = await response.json().catch(() => ({}));
    const value = payload.value ?? payload;
    if (!response.ok || value?.error) {
      const message = value?.message || `${method} ${path} failed with HTTP ${response.status}`;
      throw new Error(`${message}${includeStderr && this.stderr ? `\n${this.stderr}` : ""}`);
    }
    return value;
  }

  async #waitUntilReady() {
    const deadline = Date.now() + 15000;
    while (Date.now() < deadline) {
      if (this.failure) throw this.failure;
      try {
        await this.#request("GET", "/status", undefined, false);
        return;
      } catch {
        await new Promise((resolve) => setTimeout(resolve, 50));
      }
    }
    throw new Error(`geckodriver did not become ready\n${this.stderr}`);
  }

  #fail(error) {
    this.failure ||= error;
  }

  async #terminateProcessTree() {
    const pid = this.child?.pid;
    if (pid && this.processGroup) {
      try {
        process.kill(-pid, "SIGKILL");
      } catch (error) {
        if (error.code !== "ESRCH") throw error;
      }
    } else if (this.child && this.child.exitCode === null && this.child.signalCode === null) {
      this.child.kill("SIGKILL");
    }
    if (this.exited) await this.exited;
    if (!pid || !this.processGroup) return;
    const deadline = Date.now() + 2000;
    while (Date.now() < deadline) {
      try {
        process.kill(-pid, 0);
      } catch (error) {
        if (error.code === "ESRCH") return;
        throw error;
      }
      await new Promise((resolve) => setTimeout(resolve, 25));
    }
    throw new Error(`Gecko process group ${pid} survived shutdown`);
  }
}

export function createDriver(browser, options = {}) {
  if (browser === "firefox" || browser === "gecko") return new GeckoDriver(options);
  if (browser === "chromium") return new CdpDriver(options);
  throw new Error(`unknown Canvas browser: ${browser}`);
}
