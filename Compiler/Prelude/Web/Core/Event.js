// D-EVENT1 / D-EVENT2 / D-EVENT-CONTINUE1: Web adapter for the canonical
// event Prelude. The representation is JavaScript, but policy and lifecycle
// semantics stay aligned with JetStd::ReactiveEventWatch.
let __jet_event_next_id = 1;

function jet_event_next_id() {
  const id = __jet_event_next_id;
  __jet_event_next_id += 1;
  return id;
}

function jet_event_enum(tag, values = []) {
  return { tag, values };
}

function jet_event_ok(value) {
  return { tag: "Ok", values: [value] };
}

function jet_event_err(value) {
  return { tag: "Err", values: [value] };
}

function jet_event_tag(value, name) {
  if (value && typeof value.tag === "string") return value.tag;
  if (typeof value === "string") return value;
  throw new TypeError(`${name} must be a checked enum value`);
}

function jet_event_number(value, name) {
  const number = typeof value === "bigint" ? Number(value) : value;
  if (!Number.isFinite(number)) throw new TypeError(`${name} must be a finite integer`);
  return number;
}

function jet_event_handler(handler, name) {
  if (typeof handler !== "function") throw new TypeError(`${name} must be a closure`);
  return handler;
}

function jet_event_error_message(error) {
  if (error instanceof Error) return error.message;
  return String(error);
}

function jet_event_dispatch_failure(tag, value) {
  return jet_event_enum(tag, [value]);
}

function jet_event_policy_sync() {
  return { kind: "sync" };
}

class JetWebSubscription {
  constructor() {
    this.active_state = true;
    this.cleanup = null;
  }

  set_cleanup(cleanup) {
    this.cleanup = cleanup;
  }

  unsubscribe() {
    if (!this.active_state) return;
    this.active_state = false;
    const cleanup = this.cleanup;
    this.cleanup = null;
    if (cleanup) cleanup();
  }

  active() {
    return this.active_state;
  }
}

class JetWebEventScope {
  constructor() {
    this.id = jet_event_next_id();
    this.subscriptions = [];
    this.hard_cancellers = [];
    this.cancelled = false;
  }

  track(subscription) {
    if (this.cancelled) {
      subscription.unsubscribe();
      return subscription;
    }
    this.subscriptions = this.subscriptions.filter((item) => item.active());
    this.subscriptions.push(subscription);
    return subscription;
  }

  track_hard_cancel(owner_id, cancel) {
    if (this.cancelled) return;
    if (!this.hard_cancellers.some(([id]) => id === owner_id)) {
      this.hard_cancellers.push([owner_id, cancel]);
    }
  }

  cancel() {
    if (this.cancelled) return;
    this.cancelled = true;
    const subscriptions = this.subscriptions;
    this.subscriptions = [];
    for (const subscription of subscriptions) subscription.unsubscribe();
    const cancellers = this.hard_cancellers;
    this.hard_cancellers = [];
    for (const [, cancel] of cancellers) cancel();
  }

  active_count() {
    this.subscriptions = this.subscriptions.filter((item) => item.active());
    return this.subscriptions.length;
  }
}

class JetWebEventTrace {
  constructor(delivered, queued, dropped, summary) {
    this.delivered_value = delivered;
    this.queued_value = queued;
    this.dropped_value = dropped;
    this.summary_value = summary;
  }

  delivered() {
    return this.delivered_value;
  }

  queued() {
    return this.queued_value;
  }

  dropped() {
    return this.dropped_value;
  }

  summary() {
    return this.summary_value;
  }
}

class JetWebDispatchReport {
  constructor(accepted, state, delivered_handlers, failures, trace) {
    this.accepted_value = accepted;
    this.state_value = state;
    this.delivered_handlers_value = delivered_handlers;
    this.failures_value = failures;
    this.trace_entries = trace;
  }

  accepted() {
    return this.accepted_value;
  }

  state() {
    return jet_event_enum(this.state_value);
  }

  delivered_handlers() {
    return this.delivered_handlers_value;
  }

  failures() {
    return this.failures_value.slice();
  }

  trace() {
    return new JetWebEventTrace(
      this.delivered_handlers_value,
      this.trace_entries.filter((entry) => entry === "queued").length,
      this.state_value === "DroppedNewest" || this.state_value === "DroppedOldest" ? 1 : 0,
      this.trace_entries.join(" -> "),
    );
  }
}

function jet_event_add_listener(owner, scope, priority, once, handler) {
  const subscription = new JetWebSubscription();
  owner.listeners.push({
    id: jet_event_next_id(),
    owner_id: scope.id,
    priority,
    once,
    subscription,
    handler,
  });
  subscription.set_cleanup(() => {
    owner.listeners = owner.listeners.filter((listener) => listener.subscription !== subscription);
  });
  return scope.track(subscription);
}

class JetWebEvent {
  constructor(policy) {
    if (!policy || policy.kind !== "sync") {
      throw new TypeError("Event requires the canonical synchronous policy");
    }
    this.id = jet_event_next_id();
    this.policy = policy;
    this.listeners = [];
  }

  add(scope, priority, once, handler) {
    jet_event_handler(handler, "event handler");
    if (!(scope instanceof JetWebEventScope)) throw new TypeError("event scope is invalid");
    return jet_event_add_listener(this, scope, priority, once, handler);
  }

  on(scope, handler) {
    return this.add(scope, 0, false, handler);
  }

  once(scope, handler) {
    return this.add(scope, 0, true, handler);
  }

  on_priority(scope, priority, handler) {
    return this.add(scope, jet_event_number(priority, "event priority"), false, handler);
  }

  emit(payload) {
    const entries = this.listeners
      .filter((listener) => listener.subscription.active())
      .slice()
      .sort((left, right) => right.priority - left.priority || left.id - right.id);
    let delivered = 0;
    for (const listener of entries) {
      if (!listener.subscription.active()) continue;
      if (listener.once) listener.subscription.unsubscribe();
      listener.handler(payload);
      delivered += 1;
    }
    this.listeners = this.listeners.filter((listener) => listener.subscription.active());
    return new JetWebEventTrace(
      delivered,
      0,
      0,
      `event delivered=${delivered} queued=0 dropped=0`,
    );
  }

  listener_count() {
    this.listeners = this.listeners.filter((listener) => listener.subscription.active());
    return this.listeners.length;
  }

  trace() {
    return `listeners=${this.listener_count()} queued=0 dropped=0`;
  }
}

class JetWebAsyncEvent {
  constructor(policy, failure_policy) {
    this.id = jet_event_next_id();
    this.capacity = policy.capacity;
    this.overflow = policy.overflow;
    this.failure_policy = failure_policy;
    this.listeners = [];
    this.queued = [];
    this.blocked = [];
    this.running_entries = new Set();
    this.waiters = [];
    this.closed = false;
    this.cancelled = false;
  }

  add(scope, priority, once, handler) {
    jet_event_handler(handler, "async event handler");
    if (!(scope instanceof JetWebEventScope)) throw new TypeError("event scope is invalid");
    const subscription = jet_event_add_listener(this, scope, priority, once, handler);
    if (!subscription.active()) return subscription;
    scope.track_hard_cancel(this.id, () => this.hard_cancel());
    return subscription;
  }

  on(scope, handler) {
    return this.add(scope, 0, false, handler);
  }

  once(scope, handler) {
    return this.add(scope, 0, true, handler);
  }

  on_priority(scope, priority, handler) {
    return this.add(scope, jet_event_number(priority, "event priority"), false, handler);
  }

  wake() {
    const waiters = this.waiters;
    this.waiters = [];
    for (const resolve of waiters) resolve();
  }

  wait() {
    return new Promise((resolve) => this.waiters.push(resolve));
  }

  report_for(entry, accepted, state) {
    return new JetWebDispatchReport(
      accepted,
      state,
      0,
      [],
      entry.trace.concat(`terminal:${state}`),
    );
  }

  complete_entry(entry, accepted, state) {
    if (entry.report) return entry.report;
    entry.phase = "terminal";
    entry.accepted = accepted;
    entry.report = this.report_for(entry, accepted, state);
    entry.payload = undefined;
    this.wake();
    return entry.report;
  }

  complete_report(entry, report) {
    if (entry.report) return entry.report;
    entry.phase = "terminal";
    entry.report = report;
    entry.payload = undefined;
    this.wake();
    return report;
  }

  emit_async(payload) {
    const entry = {
      id: jet_event_next_id(),
      phase: "pending",
      accepted: false,
      payload,
      trace: [],
      report: null,
    };
    if (this.closed) {
      this.complete_entry(entry, false, "Closed");
    } else if (this.cancelled) {
      this.complete_entry(entry, false, "Cancelled");
    } else if (this.queued.length < this.capacity) {
      entry.phase = "queued";
      entry.accepted = true;
      entry.trace.push("queued");
      this.queued.push(entry);
    } else if (this.overflow === "Block") {
      entry.trace.push("pending");
      this.blocked.push(entry);
    } else if (this.overflow === "DropNewest") {
      this.complete_entry(entry, false, "DroppedNewest");
    } else {
      const oldest = this.queued.shift();
      if (oldest) this.complete_entry(oldest, true, "DroppedOldest");
      entry.phase = "queued";
      entry.accepted = true;
      entry.trace.push("queued");
      this.queued.push(entry);
    }
    this.wake();
    return jet_task_spawn(() => this.run_entry(entry));
  }

  promote(entry) {
    const index = this.blocked.indexOf(entry);
    if (index < 0 || this.queued.length >= this.capacity) return false;
    this.blocked.splice(index, 1);
    entry.phase = "queued";
    entry.accepted = true;
    entry.trace.push("queued");
    this.queued.push(entry);
    return true;
  }

  advance(entry) {
    if (entry.report) return "done";
    if (this.cancelled && entry.phase !== "terminal") {
      this.complete_entry(entry, entry.phase === "queued", "Cancelled");
      return "done";
    }
    if (entry.phase === "pending" && this.closed) {
      this.complete_entry(entry, false, "Closed");
      return "done";
    }
    if (entry.phase === "pending" && this.blocked.includes(entry)) {
      if (this.closed) {
        this.blocked.splice(this.blocked.indexOf(entry), 1);
        this.complete_entry(entry, false, "Closed");
        return "done";
      }
      this.promote(entry);
    }
    if (
      entry.phase === "queued" &&
      this.running_entries.size === 0 &&
      this.queued[0] === entry
    ) {
      this.queued.shift();
      entry.phase = "running";
      entry.trace.push("running");
      this.running_entries.add(entry);
      this.wake();
      return "run";
    }
    return "wait";
  }

  async run_entry(entry) {
    for (;;) {
      const action = this.advance(entry);
      if (action === "done") return entry.report;
      if (action === "run") return this.dispatch(entry);
      await this.wait();
    }
  }

  async dispatch(entry) {
    try {
      if (entry.report) return entry.report;
      const listeners = this.listeners
        .filter((listener) => listener.subscription.active())
        .slice()
        .sort((left, right) => right.priority - left.priority || left.id - right.id)
        .map((listener) => ({ ...listener, reserved: false }));
      for (const listener of listeners) {
        if (listener.once) {
          listener.subscription.unsubscribe();
          listener.reserved = true;
        }
      }
      const report = new JetWebDispatchReport(
        true,
        "Delivered",
        0,
        [],
        entry.trace.slice(),
      );
      for (let index = 0; index < listeners.length; index += 1) {
        const listener = listeners[index];
        if (entry.report) return entry.report;
        if (!listener.reserved && !listener.subscription.active()) continue;
        report.delivered_handlers_value += 1;
        let result;
        try {
          result = await Promise.resolve(listener.handler(entry.payload));
        } catch (error) {
          report.state_value = "HandlerFailed";
          report.failures_value.push(jet_event_dispatch_failure("Panic", jet_event_error_message(error)));
          report.trace_entries.push(`handler:${index}:panic:${jet_event_error_message(error)}`);
          break;
        }
        if (entry.report) return entry.report;
        if (result === undefined || result === null) {
          report.trace_entries.push(`handler:${index}:delivered`);
          continue;
        }
        if (result && result.tag === "Ok") {
          report.trace_entries.push(`handler:${index}:delivered`);
          continue;
        }
        if (!result || result.tag !== "Err" || !Array.isArray(result.values)) {
          throw new TypeError("async event handler must return Unit or Result");
        }
        const error = result.values[0];
        if (this.failure_policy === "StopFirst") {
          report.state_value = "HandlerFailed";
          report.failures_value.push(jet_event_dispatch_failure("Handler", error));
          report.trace_entries.push(`handler:${index}:failed`);
          break;
        }
        if (this.failure_policy === "Collect") {
          report.state_value = "HandlerFailed";
          report.failures_value.push(jet_event_dispatch_failure("Handler", error));
          report.trace_entries.push(`handler:${index}:failed`);
        } else if (this.failure_policy === "Log") {
          report.trace_entries.push(`handler:${index}:failed`);
          if (typeof console !== "undefined" && typeof console.error === "function") {
            console.error("event handler failed");
          }
        }
      }
      report.trace_entries.push(`terminal:${report.state_value}`);
      return this.complete_report(entry, report);
    } finally {
      this.running_entries.delete(entry);
      this.listeners = this.listeners.filter((listener) => listener.subscription.active());
      this.wake();
    }
  }

  hard_cancel() {
    if (this.cancelled) return;
    this.cancelled = true;
    const queued = this.queued;
    this.queued = [];
    const blocked = this.blocked;
    this.blocked = [];
    for (const entry of queued) this.complete_entry(entry, true, "Cancelled");
    for (const entry of blocked) this.complete_entry(entry, false, "Cancelled");
    for (const entry of this.running_entries) {
      this.complete_entry(entry, true, "Cancelled");
    }
    this.wake();
  }

  close() {
    if (this.closed) return;
    this.closed = true;
    const blocked = this.blocked;
    this.blocked = [];
    for (const entry of blocked) this.complete_entry(entry, false, "Closed");
    this.wake();
  }

  listener_count() {
    this.listeners = this.listeners.filter((listener) => listener.subscription.active());
    return this.listeners.length;
  }

  queued_count() {
    return this.queued.length;
  }

  running_count() {
    return this.running_entries.size;
  }

  blocked_count() {
    return this.blocked.length;
  }
}

class JetWebHook {
  constructor(fallback) {
    this.fallback = fallback;
    this.listeners = [];
  }

  add(scope, priority, once, handler) {
    jet_event_handler(handler, "hook handler");
    if (!(scope instanceof JetWebEventScope)) throw new TypeError("event scope is invalid");
    return jet_event_add_listener(this, scope, priority, once, handler);
  }

  on(scope, handler) {
    return this.add(scope, 0, false, handler);
  }

  once(scope, handler) {
    return this.add(scope, 0, true, handler);
  }

  on_priority(scope, priority, handler) {
    return this.add(scope, jet_event_number(priority, "hook priority"), false, handler);
  }

  run(payload, fallback) {
    const entries = this.listeners
      .filter((listener) => listener.subscription.active())
      .slice()
      .sort((left, right) => right.priority - left.priority || left.id - right.id);
    let result = entries.length === 0 ? fallback : this.fallback;
    for (const listener of entries) {
      if (!listener.subscription.active()) continue;
      if (listener.once) listener.subscription.unsubscribe();
      result = listener.handler(payload);
    }
    this.listeners = this.listeners.filter((listener) => listener.subscription.active());
    return result;
  }

  listener_count() {
    this.listeners = this.listeners.filter((listener) => listener.subscription.active());
    return this.listeners.length;
  }

  trace() {
    return `hook listeners=${this.listener_count()}`;
  }
}

class JetWebDecisionHook {
  constructor(policy) {
    if (policy !== "FirstCancelElseTransform") {
      throw new TypeError("unsupported decision hook policy");
    }
    this.id = jet_event_next_id();
    this.policy = policy;
    this.listeners = [];
  }

  add(scope, priority, once, handler) {
    jet_event_handler(handler, "decision hook handler");
    if (!(scope instanceof JetWebEventScope)) throw new TypeError("event scope is invalid");
    return jet_event_add_listener(this, scope, priority, once, handler);
  }

  on(scope, handler) {
    return this.add(scope, 0, false, handler);
  }

  once(scope, handler) {
    return this.add(scope, 0, true, handler);
  }

  on_priority(scope, priority, handler) {
    return this.add(scope, jet_event_number(priority, "hook priority"), false, handler);
  }

  run(payload) {
    let current = payload;
    const entries = this.listeners
      .filter((listener) => listener.subscription.active())
      .slice()
      .sort((left, right) => right.priority - left.priority || left.id - right.id);
    for (const listener of entries) {
      if (!listener.subscription.active()) continue;
      if (listener.once) listener.subscription.unsubscribe();
      const decision = listener.handler(current);
      const tag = jet_event_tag(decision, "hook decision");
      const values = Array.isArray(decision.values) ? decision.values : [];
      if (tag === "Continue") continue;
      if (tag === "Transform") {
        current = values[0];
        continue;
      }
      if (tag === "Cancel") return jet_event_enum("Cancel");
      if (tag === "Fail") return jet_event_enum("Fail", [values[0]]);
      throw new TypeError(`unknown hook decision ${tag}`);
    }
    this.listeners = this.listeners.filter((listener) => listener.subscription.active());
    return jet_event_enum("Continue", [current]);
  }

  listener_count() {
    this.listeners = this.listeners.filter((listener) => listener.subscription.active());
    return this.listeners.length;
  }
}

function jet_event_scope_new() {
  return new JetWebEventScope();
}

function jet_event_new() {
  return new JetWebEvent(jet_event_policy_sync());
}

function jet_event_with_policy(policy) {
  return new JetWebEvent(policy);
}

function jet_event_hook_new(fallback) {
  return new JetWebHook(fallback);
}

function jet_event_decision_hook_new(policy) {
  return new JetWebDecisionHook(jet_event_tag(policy, "hook policy"));
}

function jet_async_event_new(policy, failure_policy) {
  if (!policy || typeof policy !== "object") throw new TypeError("async event policy is invalid");
  const capacity = jet_event_number(policy.capacity, "async event capacity");
  if (capacity <= 0 || !Number.isInteger(capacity)) {
    return jet_event_err(jet_event_enum("InvalidCapacity"));
  }
  const overflow = jet_event_tag(policy.overflow, "event overflow policy");
  if (!["Block", "DropNewest", "DropOldest"].includes(overflow)) {
    throw new TypeError(`unknown event overflow policy ${overflow}`);
  }
  const failure = jet_event_tag(failure_policy, "event failure policy");
  if (!["StopFirst", "Collect", "Log", "Ignore"].includes(failure)) {
    throw new TypeError(`unknown event failure policy ${failure}`);
  }
  return jet_event_ok(new JetWebAsyncEvent({ capacity, overflow }, failure));
}

function jet_event_on(event, scope, handler) {
  return event.on(scope, handler);
}

function jet_event_once(event, scope, handler) {
  return event.once(scope, handler);
}

function jet_event_on_priority(event, scope, priority, handler) {
  return event.on_priority(scope, priority, handler);
}

function jet_event_emit(event, payload) {
  return event.emit(payload);
}

function jet_event_listener_count(event) {
  return event.listener_count();
}

function jet_event_trace(event) {
  return event.trace();
}

function jet_async_event_on(event, scope, handler) {
  return event.on(scope, handler);
}

function jet_async_event_once(event, scope, handler) {
  return event.once(scope, handler);
}

function jet_async_event_on_priority(event, scope, priority, handler) {
  return event.on_priority(scope, priority, handler);
}

function jet_async_event_emit(event, payload) {
  return event.emit_async(payload);
}

function jet_async_event_close(event) {
  event.close();
}

function jet_async_event_listener_count(event) {
  return event.listener_count();
}

function jet_async_event_queued_count(event) {
  return event.queued_count();
}

function jet_async_event_running_count(event) {
  return event.running_count();
}

function jet_async_event_blocked_count(event) {
  return event.blocked_count();
}

function jet_hook_on(hook, scope, handler) {
  return hook.on(scope, handler);
}

function jet_hook_once(hook, scope, handler) {
  return hook.once(scope, handler);
}

function jet_hook_on_priority(hook, scope, priority, handler) {
  return hook.on_priority(scope, priority, handler);
}

function jet_hook_run(hook, payload, fallback) {
  return hook.run(payload, fallback);
}

function jet_hook_listener_count(hook) {
  return hook.listener_count();
}

function jet_hook_trace(hook) {
  return hook.trace();
}

function jet_decision_hook_on(hook, scope, handler) {
  return hook.on(scope, handler);
}

function jet_decision_hook_once(hook, scope, handler) {
  return hook.once(scope, handler);
}

function jet_decision_hook_on_priority(hook, scope, priority, handler) {
  return hook.on_priority(scope, priority, handler);
}

function jet_decision_hook_run(hook, payload) {
  return hook.run(payload);
}

function jet_decision_hook_listener_count(hook) {
  return hook.listener_count();
}

function jet_subscription_unsubscribe(subscription) {
  subscription.unsubscribe();
}

function jet_subscription_active(subscription) {
  return subscription.active();
}

function jet_event_scope_cancel(scope) {
  scope.cancel();
}

function jet_event_scope_active_count(scope) {
  return scope.active_count();
}

function jet_event_trace_summary(trace) {
  return trace.summary();
}

function jet_event_trace_delivered(trace) {
  return trace.delivered();
}

function jet_event_trace_queued(trace) {
  return trace.queued();
}

function jet_event_trace_dropped(trace) {
  return trace.dropped();
}

function jet_dispatch_report_state(report) {
  return report.state();
}

function jet_dispatch_report_accepted(report) {
  return report.accepted();
}

function jet_dispatch_report_delivered_handlers(report) {
  return report.delivered_handlers();
}

function jet_dispatch_report_failures(report) {
  return report.failures();
}

function jet_dispatch_report_trace(report) {
  return report.trace();
}
