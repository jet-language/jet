// D-CONC-SPAWN1 / D-CONC-FAIL1: web adapter for the canonical task Prelude.
// Web has no native thread handle. The task still starts at spawn, owns one
// result, and consumes that result exactly once at join or detach.
class JetWebTask {
  constructor(work) {
    this.state = "running";
    try {
      this.result = { tag: "Ok", values: [work()] };
    } catch (error) {
      this.result = {
        tag: "Err",
        values: [{ tag: "Panicked", values: [String(error?.message ?? error)] }],
      };
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

function jet_task_detach(task) {
  task.detach();
}
