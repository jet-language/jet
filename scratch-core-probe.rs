extern crate jet_foundation;

use std::collections::BTreeSet;

fn quoted(text: &str) -> Vec<&str> {
    let mut values = Vec::new();
    let mut rest = text;
    while let Some((_, after_open)) = rest.split_once('"') {
        let Some((value, after_close)) = after_open.split_once('"') else {
            break;
        };
        values.push(value);
        rest = after_close;
    }
    values
}

fn consumer(source: &str) -> BTreeSet<(String, String)> {
    let end = source.find("pub(super) fn evaluate_method").unwrap();
    let source = &source[..end];
    let mut pairs = BTreeSet::new();
    for line in source.lines() {
        let Some((head, _)) = line.split_once("=>") else {
            continue;
        };
        let Some((_, route_and_member)) = head.split_once("CoreCallPureRoute::") else {
            continue;
        };
        let route = route_and_member
            .split(|character| matches!(character, ',' | ')' | ' '))
            .next()
            .unwrap_or_default();
        let Some((_, members)) = head.split_once(',') else {
            continue;
        };
        for member in quoted(members) {
            pairs.insert((route.to_string(), member.to_string()));
        }
    }
    pairs
}

fn main() {
    let table: BTreeSet<_> = jet_foundation::Syntax::CORE_CALLS
        .iter()
        .filter(|row| row.pure_route != jet_foundation::Syntax::CoreCallPureRoute::None)
        .map(|row| (format!("{:?}", row.pure_route), row.member.to_string()))
        .collect();
    let parity = include_str!("crates/jet-comptime/src/Comptime/CorePureParity.rs");
    let consumer = consumer(parity);
    println!("TABLE {}\nCONSUMER {}", table.len(), consumer.len());
    println!("TABLE_ONLY");
    for pair in table.difference(&consumer) {
        println!("{pair:?}");
    }
    println!("CONSUMER_ONLY");
    for pair in consumer.difference(&table) {
        println!("{pair:?}");
    }
}
