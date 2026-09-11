// D-CORE-EAGER1=A / D-LOOPMAP1=B: web collection adapters use the same eager
// collection contract as the native Prelude. The web tier supplies only the
// array marshalling; these functions own the adapter call shape.
function jet_list_map(xs, f) {
  if (xs && xs.__jet_iter) return xs.map(f);
  return xs.map((value) => f(value));
}
// Native `jet_list_fold` is left-to-right and passes the accumulator before
// each item. Arrays stay on their iterable path; a lazy view drives its
// existing one-shot pull directly rather than materialising a second array.
function jet_list_fold(xs, seed, f) {
  if (xs && xs.__jet_iter) return xs.fold(seed, f);
  let acc = seed;
  for (const value of xs) acc = f(acc, value);
  return acc;
}

// D-FRED1=A: these are only Web marshalling adapters. The compiled Prelude
// export owns seed, lanes, and tree shape through the shared Compute bridge.
function jet_list_sum_fixed_f32(xs) {
  const values = xs && xs.__jet_iter ? xs.to_list() : xs;
  return jet_compute_web_shared_reduce(values, Math.fround(0), true);
}

function jet_list_sum_fixed_f64(xs) {
  const values = xs && xs.__jet_iter ? xs.to_list() : xs;
  return jet_compute_web_shared_reduce(values, 0, false);
}
// D-FRED1=A: scalar-add folds use the same compiled fixed-order reduction
// export after placing the seed in lane zero.
function jet_list_fold_add_fixed_f32(xs, seed) {
  const values = xs && xs.__jet_iter ? xs.to_list() : xs;
  return jet_compute_web_shared_reduce(values, seed, true);
}

function jet_list_fold_add_fixed_f64(xs, seed) {
  const values = xs && xs.__jet_iter ? xs.to_list() : xs;
  return jet_compute_web_shared_reduce(values, seed, false);
}

// D-PARCAPTURE1=D / D-FRED1=A: parallel fold retains the stable 64-item
// chunk order. Each chunk and each adjacent merge goes through the compiled
// Prelude export, so this adapter does not define a second reduction tree.
function jet_list_para_fold_add_fixed(xs, seed, f32) {
  const values = xs && xs.__jet_iter ? xs.to_list() : xs;
  if (values.length === 0) return seed;
  let partials = [];
  for (let start = 0; start < values.length; start += 64) {
    partials.push(
      jet_compute_web_shared_reduce(
        values.slice(start, start + 64),
        seed,
        f32,
      ),
    );
  }
  while (partials.length > 1) {
    const next = [];
    for (let index = 0; index < partials.length; index += 2) {
      if (index + 1 >= partials.length) {
        next.push(partials[index]);
      } else {
        next.push(
          jet_compute_web_shared_reduce(
            [partials[index], partials[index + 1]],
            f32 ? Math.fround(0) : 0,
            f32,
          ),
        );
      }
    }
    partials = next;
  }
  return partials[0];
}

function jet_list_para_fold_add_fixed_f32(xs, seed) {
  return jet_list_para_fold_add_fixed(xs, seed, true);
}

function jet_list_para_fold_add_fixed_f64(xs, seed) {
  return jet_list_para_fold_add_fixed(xs, seed, false);
}

function jet_list_filter(xs, f) {
  if (xs && xs.__jet_iter) return xs.filter(f);
  return xs.filter((value) => f(value));
}

function jet_iter_map(view, f) {
  return jet_iter_source(view).map(f);
}

function jet_iter_map_mut(view, f) {
  return jet_iter_source(view).map(f);
}

function jet_iter_filter(view, f) {
  return jet_iter_source(view).filter(f);
}

// Fallible collection callbacks keep the callback's Result carrier at the

function jet_list_push(xs, value) {
  xs.push(value);
  return null;
}

function jet_list_extend(xs, other) {
  for (const value of other) xs.push(value);
  return null;
}

function jet_list_reverse(xs) {
  xs.reverse();
  return null;
}

function jet_list_sort(xs) {
  xs.sort((left, right) => {
    if (left === right) return 0;
    return left < right ? -1 : 1;
  });
  return null;
}

function jet_list_sort_desc(xs) {
  xs.sort((left, right) => {
    if (left === right) return 0;
    return left < right ? 1 : -1;
  });
  return null;
}

function jet_list_clear(xs) {
  xs.length = 0;
  return null;
}

function jet_list_try_push(xs, value) {
  xs.push(value);
  return jet_outcome_ok(null);
}

function jet_list_try_reserve(xs, additional) {
  if (Number(additional) < 0) {
    jet_runtime_stop(
      "E3010",
      "<core.collections>",
      0,
      "list reservation requires a nonnegative amount",
    );
  }
  return jet_outcome_ok(null);
}

function jet_list_pop_kernel(xs) {
  return xs.length === 0
    ? jet_option_none()
    : jet_option_some(xs.pop());
}

function jet_list_take(xs, n) {
  const value = Number(n);
  const message = jet_sequence_argument_message("take", value);
  if (message) jet_runtime_stop("E3001", "<core.collections>", 0, message);
  return xs.slice(0, value);
}

function jet_list_skip(xs, n) {
  const value = Number(n);
  const message = jet_sequence_argument_message("skip", value);
  if (message) jet_runtime_stop("E3001", "<core.collections>", 0, message);
  return xs.slice(value);
}

function jet_list_insert(xs, index, value) {
  const position = Number(index);
  if (!Number.isSafeInteger(position) || position < 0 || position > xs.length) {
    jet_runtime_stop(
      "E3010",
      "<core.collections>",
      0,
      `the list has ${xs.length} items, so position ${index} doesn't exist`,
    );
  }
  xs.splice(position, 0, value);
  return null;
}

function jet_list_remove_value(xs, value) {
  const position = xs.findIndex((item) => item === value);
  return position < 0
    ? jet_option_none()
    : jet_option_some(xs.splice(position, 1)[0]);
}

function jet_list_remove_slot(xs, index) {
  const position = Number(index);
  if (!Number.isSafeInteger(position) || position < 0 || position >= xs.length) {
    jet_runtime_stop(
      "E3010",
      "<core.collections>",
      0,
      `the list has ${xs.length} items, so position ${index} doesn't exist`,
    );
  }
  return jet_option_some(xs.splice(position, 1)[0]);
}

function jet_map_insert(map, key, value) {
  map.set(key, value);
  return null;
}

function jet_map_add_new(map, key, value) {
  if (map.has(key)) return false;
  map.set(key, value);
  return true;
}

function jet_map_try_insert(map, key, value) {
  const previous = map.has(key)
    ? jet_option_some(map.get(key))
    : jet_option_none();
  map.set(key, value);
  return jet_outcome_ok(previous);
}

function jet_map_pop_kernel(map, key) {
  if (!map.has(key)) return jet_option_none();
  const value = map.get(key);
  map.delete(key);
  return jet_option_some(value);
}

function jet_map_pop_first(map) {
  const first = map.entries().next();
  if (first.done) return jet_option_none();
  map.delete(first.value[0]);
  return jet_option_some(first.value[1]);
}
// adapter boundary. A successful callback contributes its payload; the first
// Err is returned unchanged.
function jet_list_try_map(xs, f) {
  const values = xs && xs.__jet_iter ? xs.to_list() : xs;
  const out = [];
  for (const value of values) {
    const result = f(value);
    if (result && result.tag === "Err") return result;
    out.push(result && result.tag === "Ok" ? result.values[0] : result);
  }
  return { tag: "Ok", values: [out] };
}

function jet_list_try_filter(xs, f) {
  const values = xs && xs.__jet_iter ? xs.to_list() : xs;
  const out = [];
  for (const value of values) {
    const result = f(value);
    if (result && result.tag === "Err") return result;
    const keep = result && result.tag === "Ok" ? result.values[0] : result;
    if (keep) out.push(value);
  }
  return { tag: "Ok", values: [out] };
}

function jet_iter_try_map(view, f) {
  const source = jet_iter_source(view);
  const out = [];
  for (;;) {
    const step = source.next();
    if (step.done) break;
    const result = f(step.value);
    if (result && result.tag === "Err") return result;
    out.push(result && result.tag === "Ok" ? result.values[0] : result);
  }
  return jet_outcome_ok(jet_iter_from_vec(out));
}

function jet_iter_try_filter(view, f) {
  const source = jet_iter_source(view);
  const out = [];
  for (;;) {
    const step = source.next();
    if (step.done) break;
    const result = f(step.value);
    if (result && result.tag === "Err") return result;
    const keep = result && result.tag === "Ok" ? result.values[0] : result;
    if (keep) out.push(step.value);
  }
  return jet_outcome_ok(jet_iter_from_vec(out));
}

function jet_sequence_argument_message(method, value) {
  if ((method === "take" || method === "skip") && value < 0)
    return "sequence count must be nonnegative";
  if (method === "step_by" && value <= 0)
    return "step_by requires a positive step";
  if (method === "chunks" && value <= 0)
    return "chunks requires a positive size";
  if (method === "windows" && value <= 0)
    return "windows requires a positive size";
  return null;
}

// The JS tier keeps the same explicit deferred boundary as native tiers.
// Each view is one-shot: a second drive is a use-after-drive.
function jet_iter_lazy(source) {
  const iterator = typeof source === "function" ? source() : source;
  const input = iterator && typeof iterator.next === "function"
    ? iterator
    : iterator?.[Symbol.iterator]?.();
  if (!input || typeof input.next !== "function") {
    throw new TypeError("lazy collection source is not iterable");
  }
  let closed = false;
  const next = () => {
    if (closed) {
      jet_runtime_stop(
        "E3001",
        "<core.collections>",
        0,
        "lazy collection view was already consumed",
      );
    }
    const step = input.next();
    if (step.done) closed = true;
    return step;
  };
  const view = {
    __jet_iter: true,
    next,
    [Symbol.iterator]() {
      return this;
    },
    map(f) {
      return jet_iter_lazy((function* () {
        for (;;) {
          const step = next();
          if (step.done) return;
          yield f(step.value);
        }
      })());
    },
    filter(f) {
      return jet_iter_lazy((function* () {
        for (;;) {
          const step = next();
          if (step.done) return;
          if (f(step.value)) yield step.value;
        }
      })());
    },
    skip(n) {
      const value = Number(n);
      const message = jet_sequence_argument_message("skip", value);
      if (message) jet_runtime_stop("E3001", "<core.collections>", 0, message);
      return jet_iter_lazy((function* () {
        let remaining = value;
        while (remaining > 0) {
          const step = next();
          if (step.done) return;
          remaining -= 1;
        }
        for (;;) {
          const step = next();
          if (step.done) return;
          yield step.value;
        }
      })());
    },
    take(n) {
      const value = Number(n);
      const message = jet_sequence_argument_message("take", value);
      if (message) jet_runtime_stop("E3001", "<core.collections>", 0, message);
      return jet_iter_lazy((function* () {
        let remaining = value;
        while (remaining > 0) {
          const step = next();
          if (step.done) return;
          yield step.value;
          remaining -= 1;
        }
      })());
    },
    step_by(n) {
      const value = Number(n);
      const message = jet_sequence_argument_message("step_by", value);
      if (message) jet_runtime_stop("E3001", "<core.collections>", 0, message);
      return jet_iter_lazy((function* () {
        for (;;) {
          const step = next();
          if (step.done) return;
          yield step.value;
          for (let skipped = 1; skipped < value; skipped += 1) {
            const skipped_step = next();
            if (skipped_step.done) return;
          }
        }
      })());
    },
    dedup() {
      return jet_iter_lazy((function* () {
        let has_previous = false;
        let previous;
        for (;;) {
          const step = next();
          if (step.done) return;
          if (has_previous && previous === step.value) continue;
          previous = step.value;
          has_previous = true;
          yield step.value;
        }
      })());
    },
    chunks(n) {
      const value = Number(n);
      const message = jet_sequence_argument_message("chunks", value);
      if (message) jet_runtime_stop("E3001", "<core.collections>", 0, message);
      return jet_iter_lazy((function* () {
        let done = false;
        while (!done) {
          const chunk = [];
          for (let index = 0; index < value; index += 1) {
            const step = next();
            if (step.done) {
              done = true;
              break;
            }
            chunk.push(step.value);
          }
          if (chunk.length === 0) return;
          yield chunk;
        }
      })());
    },
    fold(seed, f) {
      let acc = seed;
      for (const value of view) acc = f(acc, value);
      return acc;
    },
    to_list() {
      const values = [];
      for (const value of view) values.push(value);
      return values;
    },
  };
  return view;
}

function jet_iter_source(value) {
  return value && value.__jet_iter ? value : jet_iter_from_vec(value);
}

function jet_iter_from_vec(xs) {
  return jet_iter_lazy(xs.slice()[Symbol.iterator]());
}

function jet_iter_to_list(view) {
  return view && view.__jet_iter ? view.to_list() : Array.from(view);
}

function jet_iter_collect(view) {
  return jet_iter_to_list(view);
}

function jet_iter_take(view, n) {
  const value = Number(n);
  const message = jet_sequence_argument_message("take", value);
  if (message) jet_runtime_stop("E3001", "<core.collections>", 0, message);
  return jet_iter_source(view).take(value);
}

function jet_iter_skip(view, n) {
  const value = Number(n);
  const message = jet_sequence_argument_message("skip", value);
  if (message) jet_runtime_stop("E3001", "<core.collections>", 0, message);
  return jet_iter_source(view).skip(value);
}

function jet_iter_step_by(view, n) {
  const value = Number(n);
  const message = jet_sequence_argument_message("step_by", value);
  if (message) jet_runtime_stop("E3001", "<core.collections>", 0, message);
  return jet_iter_source(view).step_by(value);
}

function jet_iter_dedup(view) {
  return jet_iter_source(view).dedup();
}

function jet_iter_chunks(view, n) {
  const value = Number(n);
  const message = jet_sequence_argument_message("chunks", value);
  if (message) jet_runtime_stop("E3001", "<core.collections>", 0, message);
  return jet_iter_source(view).chunks(value);
}

function jet_iter_first(view) {
  if (view && view.__jet_iter) {
    const step = view.next();
    return step.done ? jet_option_none() : jet_option_some(step.value);
  }
  return view.length === 0 ? jet_option_none() : jet_option_some(view[0]);
}

function jet_iter_empty() {
  return jet_iter_lazy([][Symbol.iterator]());
}

function jet_iter_some(view) {
  return jet_iter_source(view).map((value) => jet_option_some(value));
}

function jet_iter_indexes(n) {
  const limit = BigInt(n);
  return jet_iter_lazy((function* () {
    for (let index = 0n; index < (limit > 0n ? limit : 0n); index += 1n) yield index;
  })());
}

function jet_iter_zip(a, b, f) {
  const left = jet_iter_source(a);
  const right = jet_iter_source(b);
  return jet_iter_lazy((function* () {
    for (;;) {
      const first = left.next();
      if (first.done) return;
      const second = right.next();
      if (second.done) return;
      yield f(first.value, second.value);
    }
  })());
}

function jet_iter_zip_strict(a, b, f) {
  const left = jet_iter_source(a);
  const right = jet_iter_source(b);
  return jet_iter_lazy((function* () {
    for (;;) {
      const first = left.next();
      const second = right.next();
      if (first.done !== second.done) {
        jet_runtime_stop("E3001", "<core.collections>", 0, "zip length mismatch");
      }
      if (first.done) return;
      yield f(first.value, second.value);
    }
  })());
}

function jet_iter_zip_pad(a, b, fill_a, fill_b, f) {
  const left = jet_iter_source(a);
  const right = jet_iter_source(b);
  return jet_iter_lazy((function* () {
    for (;;) {
      const first = left.next();
      const second = right.next();
      if (first.done && second.done) return;
      yield f(
        first.done ? jet_copy_value(fill_a, undefined, __jet_copy_type_facts) : first.value,
        second.done ? jet_copy_value(fill_b, undefined, __jet_copy_type_facts) : second.value,
      );
    }
  })());
}
