//! Debugger projection of the runtime-owned Event observation sequence.
//! Runtime records are payload-free; this consumer accepts only the closed
//! schema and preserves every numeric/runtime enum fact exactly.

use jet_foundation::JSON::{json_get, json_str, parse_json, JsonValue};

const MAX_SNAPSHOT_BYTES: usize = 1024 * 1024;
const MAX_EVENTS: usize = 256;

pub fn render(snapshot: &str) -> Result<String, String> {
    if snapshot.len() > MAX_SNAPSHOT_BYTES {
        return Err("runtime observation exceeds the 1 MiB debugger limit".to_string());
    }
    let root = parse_json(snapshot)
        .map_err(|()| "runtime observation is not valid JSON".to_string())?;
    let events = match json_get(&root, "event_observations") {
        Some(JsonValue::Array(events)) if events.len() <= MAX_EVENTS => events,
        Some(JsonValue::Array(_)) => {
            return Err("runtime event observation exceeds the 256-record limit".to_string())
        }
        _ => return Err("runtime observation has no event sequence".to_string()),
    };

    let mut previous = 0;
    let mut lines = Vec::with_capacity(events.len());
    for event in events {
        let JsonValue::Object(fields) = event else {
            return Err("runtime event observation is not an object".to_string());
        };
        const KEYS: [&str; 15] = [
            "sequence",
            "source",
            "event_id",
            "owner_id",
            "subscription_id",
            "dispatch_id",
            "lifecycle",
            "queued",
            "blocked",
            "running",
            "capacity",
            "overflow",
            "priority",
            "failure",
            "terminal",
        ];
        if fields.len() != KEYS.len() || fields.keys().any(|key| !KEYS.contains(&key.as_str())) {
            return Err("runtime event observation contains an unsafe or unknown field".to_string());
        }
        let sequence = unsigned(event, "sequence")?;
        if sequence <= previous {
            return Err("runtime event observation sequence is not strictly increasing".to_string());
        }
        previous = sequence;
        let source = closed(event, "source", &["Event", "AsyncEvent", "DecisionHook"])?;
        let event_id = unsigned(event, "event_id")?;
        let owner_id = unsigned(event, "owner_id")?;
        let subscription_id = unsigned(event, "subscription_id")?;
        let dispatch_id = unsigned(event, "dispatch_id")?;
        let lifecycle = closed(
            event,
            "lifecycle",
            &[
                "Subscribed",
                "Removed",
                "DispatchStarted",
                "Queued",
                "Backpressured",
                "Running",
                "HandlerStarted",
                "HandlerDelivered",
                "HandlerFailed",
                "HandlerContinue",
                "HandlerTransform",
                "HandlerCancel",
                "HandlerFail",
                "Terminal",
            ],
        )?;
        let queued = count(event, "queued")?;
        let blocked = count(event, "blocked")?;
        let running = count(event, "running")?;
        let capacity = unsigned(event, "capacity")?;
        let overflow = closed(event, "overflow", &["None", "Block", "DropNewest", "DropOldest"])?;
        let priority = integer(event, "priority")?;
        let failure = closed(event, "failure", &["None", "Handler", "Panic"])?;
        let terminal = closed(
            event,
            "terminal",
            &[
                "None",
                "Delivered",
                "HandlerFailed",
                "DroppedNewest",
                "DroppedOldest",
                "Closed",
                "Cancelled",
                "DeadlineExceeded",
                "Continue",
                "Cancel",
                "Fail",
            ],
        )?;
        lines.push(format!(
            "sequence={sequence} source={source} event={event_id} owner={owner_id} subscription={subscription_id} dispatch={dispatch_id} lifecycle={lifecycle} queued={queued} blocked={blocked} running={running} capacity={capacity} overflow={overflow} priority={priority} failure={failure} terminal={terminal}"
        ));
    }
    Ok(lines.join("\n"))
}

fn integer(object: &JsonValue, key: &str) -> Result<i64, String> {
    match json_get(object, key) {
        Some(JsonValue::Number(value)) => Ok(*value),
        _ => Err(format!("runtime event observation has invalid `{key}`")),
    }
}

fn unsigned(object: &JsonValue, key: &str) -> Result<i64, String> {
    let value = integer(object, key)?;
    (value >= 0)
        .then_some(value)
        .ok_or_else(|| format!("runtime event observation has negative `{key}`"))
}

fn count(object: &JsonValue, key: &str) -> Result<i64, String> {
    let value = integer(object, key)?;
    (value >= -1)
        .then_some(value)
        .ok_or_else(|| format!("runtime event observation has invalid `{key}`"))
}

fn closed<'a>(object: &'a JsonValue, key: &str, allowed: &[&str]) -> Result<&'a str, String> {
    let value = json_get(object, key)
        .and_then(json_str)
        .ok_or_else(|| format!("runtime event observation has invalid `{key}`"))?;
    allowed
        .contains(&value)
        .then_some(value)
        .ok_or_else(|| format!("runtime event observation has unknown `{key}` value"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECORD: &str = "{\"sequence\":1,\"source\":\"AsyncEvent\",\"event_id\":2,\"owner_id\":3,\"subscription_id\":4,\"dispatch_id\":5,\"lifecycle\":\"HandlerFailed\",\"queued\":1,\"blocked\":0,\"running\":1,\"capacity\":8,\"overflow\":\"DropNewest\",\"priority\":17,\"failure\":\"Handler\",\"terminal\":\"None\"}";

    #[test]
    fn preserves_closed_runtime_facts() {
        let snapshot = format!("{{\"event_observations\":[{RECORD}]}}");
        assert_eq!(
            render(&snapshot).unwrap(),
            "sequence=1 source=AsyncEvent event=2 owner=3 subscription=4 dispatch=5 lifecycle=HandlerFailed queued=1 blocked=0 running=1 capacity=8 overflow=DropNewest priority=17 failure=Handler terminal=None"
        );
    }

    #[test]
    fn rejects_payload_fields_and_unbounded_sequences() {
        let unsafe_record = RECORD.replace(
            "\"terminal\":\"None\"",
            "\"terminal\":\"None\",\"payload\":\"secret\"",
        );
        assert!(render(&format!("{{\"event_observations\":[{unsafe_record}]}}"))
            .unwrap_err()
            .contains("unsafe or unknown"));
        let too_many = std::iter::repeat_n(RECORD, 257).collect::<Vec<_>>().join(",");
        assert!(render(&format!("{{\"event_observations\":[{too_many}]}}"))
            .unwrap_err()
            .contains("256-record"));
    }
}
