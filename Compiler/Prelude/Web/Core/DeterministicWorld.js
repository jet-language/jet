// D-TEST-WORLD1=A: Web uses the same scoped deterministic-world contract as
// native tiers. The world is an explicit mutable capability; host clock reads
// consult the active scope through __jet_deterministic_world.
class JetWebDeterministicWorld {
  constructor() {
    this.monotonic_ns = 0n;
    this.wall_origin_ms = 0n;
    this.rng_state = 0x4d595df4d0f33173n;
    this.next_sequence = 0n;
    this.timers = [];
    this.history = [];
    this.budget = 100000;
    this.live_tasks = 0;
  }

  now() {
    return this.wall_origin_ms + this.monotonic_ns / 1000000n;
  }

  schedule_timer(duration_ns, resolve) {
    const duration = BigInt(duration_ns);
    if (duration <= 0n) {
      resolve();
      return;
    }
    const deadline = this.monotonic_ns + duration;
    const sequence = this.next_sequence;
    this.next_sequence += 1n;
    this.timers.push({ deadline, sequence, resolve });
    this.history.push(`timer:register:${deadline}:${sequence}`);
  }

  advance(duration_ns) {
    const duration = BigInt(duration_ns);
    if (duration < 0n) {
      throw new Error("deterministic world cannot move time backwards; advance requires a non-negative duration");
    }
    const target = this.monotonic_ns + duration;
    let steps = 0;
    for (;;) {
      if (++steps > this.budget) {
        throw new Error("deterministic world execution budget exhausted while advancing");
      }
      let index = -1;
      for (let candidate = 0; candidate < this.timers.length; candidate += 1) {
        const timer = this.timers[candidate];
        if (timer.deadline > target) continue;
        if (
          index < 0
          || timer.deadline < this.timers[index].deadline
          || (
            timer.deadline === this.timers[index].deadline
            && timer.sequence < this.timers[index].sequence
          )
        ) {
          index = candidate;
        }
      }
      if (index < 0) {
        this.monotonic_ns = target;
        break;
      }
      const [timer] = this.timers.splice(index, 1);
      this.monotonic_ns = timer.deadline;
      this.history.push(`timer:${timer.deadline}:${timer.sequence}`);
      timer.resolve();
    }
    this.history.push(`advance:${duration}`);
    return this.now();
  }
  wait_idle() {
    let steps = 0;
    for (;;) {
      if (++steps > this.budget) {
        throw new Error("deterministic world execution budget exhausted while waiting for idle");
      }
      let index = -1;
      for (let candidate = 0; candidate < this.timers.length; candidate += 1) {
        const timer = this.timers[candidate];
        if (timer.deadline > this.monotonic_ns) continue;
        if (
          index < 0
          || timer.deadline < this.timers[index].deadline
          || (
            timer.deadline === this.timers[index].deadline
            && timer.sequence < this.timers[index].sequence
          )
        ) {
          index = candidate;
        }
      }
      if (index < 0) return;
      const [timer] = this.timers.splice(index, 1);
      this.history.push(`timer:${timer.deadline}:${timer.sequence}`);
      timer.resolve();
    }
  }

}

function jet_world_reject_uncontrolled_effect(effect) {
  if (!globalThis.__jet_deterministic_world) return;
  throw new Error(
    `E3404: deterministic world has no controlled provider for ${String(effect)}`,
  );
}

function jet_testing_world(callback) {
  if (typeof callback !== "function") {
    throw new Error("testing.world requires a callback");
  }
  if (globalThis.__jet_deterministic_world) {
    throw new Error("deterministic world nesting is ambiguous; use the parent world explicitly");
  }
  const world = new JetWebDeterministicWorld();
  const previous = globalThis.__jet_deterministic_world;
  globalThis.__jet_deterministic_world = world;
  const restore = () => {
    if (previous === undefined) {
      delete globalThis.__jet_deterministic_world;
    } else {
      globalThis.__jet_deterministic_world = previous;
    }
  };
  const finish = () => {
    if (world.live_tasks !== 0) {
      throw new Error("deterministic world scope exited with live descendants; cancel and join every task");
    }
    restore();
  };
  try {
    const result = callback(world);
    if (result && typeof result.then === "function") {
      return Promise.resolve(result).then(
        (value) => {
          try {
            finish();
          } catch (error) {
            restore();
            throw error;
          }
          return value;
        },
        (error) => {
          restore();
          throw error;
        },
      );
    }
    finish();
    return result;
  } catch (error) {
    restore();
    throw error;
  }
}

function jet_world_checked(world) {
  if (!(world instanceof JetWebDeterministicWorld)) {
    throw new Error("deterministic-world method received an invalid world");
  }
  return world;
}

function jet_world_now(world) {
  return jet_world_checked(world).now();
}

function jet_world_advance(world, duration_ns) {
  return jet_world_checked(world).advance(duration_ns);
}

function jet_world_wait_idle(world) {
  jet_world_checked(world).wait_idle();
}

function jet_world_history(world) {
  return jet_world_checked(world).history.join("\n");
}
