// D-CONC-CHAN1 / D-CONC-CHAN2 / I9: the JS adapter for the canonical channel
// and readiness Prelude. Web.rs only marshals TIR values to these doors.

function jet_web_closed_result() {
  return { tag: "Err", values: [{ tag: "Closed", values: [] }] };
}

class JetWebChannel {
  constructor(capacity = null) {
    this.capacity = capacity == null ? null : Number(BigInt(capacity) <= 0n ? 1n : BigInt(capacity));
    this.queue = [];
    this.closed = false;
    this.receivers = new Set();
    this.senders = [];
    this.selectWaiters = new Set();
  }

  notify() {
    for (const waiter of [...this.selectWaiters]) waiter();
  }

  receive() {
    if (this.queue.length !== 0) {
      const value = this.queue.shift();
      this.promoteSender();
      this.notify();
      return Promise.resolve({ tag: "Ok", values: [value] });
    }
    if (this.closed) return Promise.resolve(jet_web_closed_result());
    return new Promise((resolve) => {
      this.receivers.add(resolve);
    });
  }

  tryReceive() {
    if (this.queue.length !== 0) {
      const value = this.queue.shift();
      this.promoteSender();
      this.notify();
      return { ready: true, value };
    }
    return { ready: false, closed: this.closed };
  }

  promoteSender() {
    if (this.closed || this.senders.length === 0) return;
    if (this.receivers.size !== 0) {
      const resolve = this.receivers.values().next().value;
      this.receivers.delete(resolve);
      const sender = this.senders.shift();
      resolve({ tag: "Ok", values: [sender.value] });
      sender.resolve();
      return;
    }
    if (this.capacity == null || this.queue.length < this.capacity) {
      const sender = this.senders.shift();
      this.queue.push(sender.value);
      sender.resolve();
    }
  }

  send(value) {
    if (this.closed) return Promise.resolve();
    if (this.receivers.size !== 0) {
      const resolve = this.receivers.values().next().value;
      this.receivers.delete(resolve);
      resolve({ tag: "Ok", values: [value] });
      this.notify();
      return Promise.resolve();
    }
    if (this.capacity == null || this.queue.length < this.capacity) {
      this.queue.push(value);
      this.notify();
      return Promise.resolve();
    }
    return new Promise((resolve) => {
      this.senders.push({ value, resolve });
    });
  }

  close() {
    if (this.closed) return;
    this.closed = true;
    for (const resolve of this.receivers) resolve(jet_web_closed_result());
    this.receivers.clear();
    for (const sender of this.senders) sender.resolve();
    this.senders = [];
    this.notify();
  }

  registerSelect(waiter) {
    this.selectWaiters.add(waiter);
  }

  removeSelect(waiter) {
    this.selectWaiters.delete(waiter);
  }
}

class JetWebSender {
  constructor(channel) {
    this.channel = channel;
  }
}

class JetWebReceiver {
  constructor(channel) {
    this.channel = channel;
  }
}

function jet_channel_new() {
  const channel = new JetWebChannel();
  return { sender: new JetWebSender(channel), receiver: new JetWebReceiver(channel) };
}

function jet_channel_bounded(capacity) {
  const channel = new JetWebChannel(capacity);
  return { sender: new JetWebSender(channel), receiver: new JetWebReceiver(channel) };
}

function jet_channel_send(sender, value) {
  return sender.channel.send(value);
}

function jet_channel_receive(receiver) {
  return receiver.channel.receive();
}

function jet_channel_close(endpoint) {
  endpoint.channel.close();
}

function jet_web_duration_ns(value) {
  let current = value;
  if (current != null && current.tag === "Ok") current = current.values[0];
  return BigInt(current);
}

function jet_web_duration_ms(value) {
  const ns = jet_web_duration_ns(value);
  if (ns <= 0n) return 0;
  const ms = Number(ns / 1000000n);
  return Number.isFinite(ms) ? Math.min(2147483647, Math.max(0, ms)) : 2147483647;
}

function jet_scheduler_select_probe(receivers, durations, started) {
  for (let index = 0; index < receivers.length; index += 1) {
    const result = receivers[index].channel.tryReceive();
    if (result.ready) {
      return [BigInt(index), { tag: "Some", values: [result.value] }];
    }
  }
  for (let index = 0; index < durations.length; index += 1) {
    if (durations[index].ms === 0 || performance.now() - started >= durations[index].ms) {
      return [BigInt(receivers.length + index), { tag: "None", values: [] }];
    }
  }
  if (durations.length === 0 && receivers.length !== 0
      && receivers.every((receiver) => receiver.channel.closed && receiver.channel.queue.length === 0)) {
    return null;
  }
  return undefined;
}

function jet_scheduler_select(receivers, duration_values) {
  if (receivers.length === 0 && duration_values.length === 0) {
    throw new Error("select: no arms registered");
  }
  const durations = duration_values.map((value) => ({ ms: jet_web_duration_ms(value) }));
  const started = performance.now();
  const immediate = jet_scheduler_select_probe(receivers, durations, started);
  if (immediate !== undefined) {
    if (immediate === null) throw new Error("select closed");
    return Promise.resolve(immediate);
  }
  return new Promise((resolve, reject) => {
    let active = true;
    const timers = [];
    const waiters = [];
    const cleanup = () => {
      if (!active) return;
      active = false;
      for (let index = 0; index < receivers.length; index += 1) {
        receivers[index].channel.removeSelect(waiters[index]);
      }
      for (const timer of timers) clearTimeout(timer);
    };
    const check = () => {
      if (!active) return;
      const result = jet_scheduler_select_probe(receivers, durations, started);
      if (result === undefined) return;
      cleanup();
      if (result === null) reject(new Error("select closed"));
      else resolve(result);
    };
    for (const receiver of receivers) {
      const waiter = check;
      waiters.push(waiter);
      receiver.channel.registerSelect(waiter);
    }
    for (const duration of durations) {
      timers.push(setTimeout(check, duration.ms));
    }
    // Registration can race a send or close. Recheck after every waiter is in.
    check();
  });
}

function jet_scheduler_try_select(receivers, duration_values) {
  if (receivers.length === 0 && duration_values.length === 0) {
    throw new Error("select: no arms registered");
  }
  const durations = duration_values.map((value) => ({ ms: jet_web_duration_ms(value) }));
  const result = jet_scheduler_select_probe(receivers, durations, performance.now());
  return result === undefined || result === null
    ? [-1n, { tag: "None", values: [] }]
    : result;
}
