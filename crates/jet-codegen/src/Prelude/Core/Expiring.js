// D-SHAPE-CTORVERB1=C: the Web representation of checked expiring values.
// The clock and deadline are carried by the value; MIR decides which route is
// used and supplies the already-checked operands.
function jet_expiring_now(clock) {
  if (typeof clock === "function") return Number(clock());
  if (clock != null && typeof clock === "object" && "now" in clock) {
    return Number(clock.now);
  }
  return Number(clock);
}

function jet_expiring_new(value, ttlMs, clock) {
  const now = jet_expiring_now(clock);
  return {
    value,
    deadline: now + Number(ttlMs),
    clock,
    secret: false,
  };
}

function jet_expiring_secret_new(value, ttlMs, clock) {
  const now = jet_expiring_now(clock);
  return {
    value,
    deadline: now + Number(ttlMs),
    clock,
    secret: true,
  };
}

function jet_expiring_get(expiring, now) {
  if (expiring == null || typeof expiring !== "object" || !("deadline" in expiring)) {
    throw new Error("invalid Web expiring value");
  }
  if (jet_expiring_now(now) > Number(expiring.deadline)) {
    expiring.value = undefined;
    return { tag: "Err", values: [{ tag: "Expired", values: [] }] };
  }
  return { tag: "Ok", values: [expiring.value] };
}

function jet_expiring_secret_with(expiring, callback) {
  if (
    expiring == null ||
    typeof expiring !== "object" ||
    expiring.secret !== true ||
    !("deadline" in expiring)
  ) {
    throw new Error("invalid Web ExpiringSecret value");
  }
  if (typeof callback !== "function") throw new Error("invalid Web ExpiringSecret callback");
  if (expiring.value === undefined || jet_expiring_now(expiring.clock) > Number(expiring.deadline)) {
    expiring.value = undefined;
    return { tag: "Err", values: [{ tag: "Expired", values: [] }] };
  }
  return { tag: "Ok", values: [callback(expiring.value)] };
}
