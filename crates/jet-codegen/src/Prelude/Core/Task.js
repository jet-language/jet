// D-CONC-SPAWN1 / D-CONC-FAIL1: web adapter for the canonical task Prelude.
// Web has no native thread handle. The task still starts at spawn, owns one
// result, and consumes that result exactly once at join or detach.
class JetWebTask {
  constructor(work) {
    this.state = "running";
    this.world = globalThis.__jet_deterministic_world;
    this.finished = false;
    if (this.world) {
      this.world.live_tasks += 1;
      this.world.history.push("task:spawn");
    }
    let work_result;
    if (this.world) {
      try {
        // Start controlled work immediately so its virtual timers are
        // registered before the world advances its clock.
        work_result = work();
      } catch (error) {
        work_result = Promise.reject(error);
      }
    } else {
      work_result = Promise.resolve().then(() => work());
    }
    this.result = Promise.resolve(work_result)
      .then(
        (value) => ({ tag: "Ok", values: [value] }),
        (error) => ({
          tag: "Err",
          values: [{ tag: "Panicked", values: [String(error?.message ?? error)] }],
        }),
      )
      .finally(() => this.finish());
  }

  finish() {
    if (this.finished) return;
    this.finished = true;
    if (this.world) {
      this.world.live_tasks -= 1;
      this.world.history.push("task:finish");
    }
  }

  join() {
    if (this.state !== "running") throw new Error("task already consumed");
    this.state = "joined";
    return this.result;
  }

  detach() {
    if (this.state !== "running") throw new Error("task already consumed");
    this.state = "detached";
  }
}

function jet_task_spawn(work) {
  return new JetWebTask(work);
}

function jet_task_join(task) {
  return task.join();
}
async function jet_task_join_result(task) {
  const joined = await jet_task_join(task);
  if (joined?.tag !== "Ok") return joined;
  const value = joined.values[0];
  if (value?.tag === "Ok") return { tag: "Ok", values: [value.values[0]] };
  if (value?.tag === "Err") {
    return {
      tag: "Err",
      values: [{
        tag: "Panicked",
        values: [`task body returned an error: ${typeof jet_debug === "function" ? jet_debug(value.values[0]) : String(value.values[0])}`],
      }],
    };
  }
  return {
    tag: "Err",
    values: [{ tag: "Panicked", values: ["task body returned an invalid result carrier"] }],
  };
}

function jet_task_detach(task) {
  task.detach();
}
