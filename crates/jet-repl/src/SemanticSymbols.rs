//! Shared builtin semantic-symbol catalog for REPL, LSP, and `jet ?`.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub identity: &'static str,
    pub module: &'static str,
    pub owner: Option<&'static str>,
    pub member: &'static str,
    pub signature: &'static str,
    pub summary: &'static str,
    pub example: &'static str,
    pub provenance: &'static str,
}

macro_rules! member {
    ($owner:literal, $name:literal, $signature:literal, $summary:literal) => {
        Symbol {
            identity: concat!($owner, ".", $name), module: "core.collections",
            owner: Some($owner), member: $name, signature: $signature,
            summary: $summary, example: concat!($owner, ".", $name, "(...)"),
            provenance: "builtin",
        }
    };
}

pub const SYMBOLS: &[Symbol] = &[
    member!("List", "len", "List.len() -> Int", "Number of items."),
    member!("List", "is_empty", "List.is_empty() -> Bool", "True when there are no items."),
    member!("List", "push", "List.push(item: T)", "Appends an item to the end."),
    member!("List", "pop", "List.pop() -> T?", "Removes and returns the last item, if any."),
    member!("List", "get", "List.get(i: Int) -> T?", "The item at index i, if in bounds."),
    member!("List", "first", "List.first() -> T?", "The first item, if any."),
    member!("List", "last", "List.last() -> T?", "The last item, if any."),
    member!("List", "contains", "List.contains(item: T) -> Bool", "True when item appears in the list."),
    member!("List", "index_of", "List.index_of(item: T) -> Int?", "Index of the first matching item, if any."),
    member!("List", "join", "List.join(sep: String) -> String", "Joins string items with sep."),
    member!("List", "sum", "List.sum() -> T", "Sum of all items."),
    member!("List", "product", "List.product() -> T", "Product of all items."),
    member!("List", "min", "List.min() -> T?", "The smallest item, if any."),
    member!("List", "max", "List.max() -> T?", "The largest item, if any."),
    member!("List", "map", "List.map(f: fn(T) -> R) -> [R]", "Transforms each item with f."),
    member!("List", "filter", "List.filter(f: fn(T) -> Bool) -> List<T>", "Keeps items where f(item) is true."),
    member!("List", "filter_map", "List.filter_map(f: fn(T) -> V?) -> [V]", "Maps then drops failures — keeps only successes."),
    member!("List", "each", "List.each(f: fn(T))", "Runs f once per item, for its side effects."),
    member!("List", "find", "List.find(f: fn(T) -> Bool) -> T?", "The first item where f(item) is true, if any."),
    member!("List", "any", "List.any(f: fn(T) -> Bool) -> Bool", "True if f is true for at least one item."),
    member!("List", "all", "List.all(f: fn(T) -> Bool) -> Bool", "True if f is true for every item."),
    member!("List", "sort_by", "List.sort_by(key: fn(T) -> K)", "Sorts in place by the key f extracts."),
    member!("List", "reduce", "List.reduce(init: R, f: fn(R, T) -> R) -> R", "Folds items into one value, starting from init."),
    member!("List", "fold", "List.fold(init: R, f: fn(R, T) -> R) -> R", "Folds items into one value, starting from init."),
    member!("List", "reverse", "List.reverse()", "Reverses the list in place."),
    member!("List", "sort", "List.sort()", "Sorts the list in place."),
    member!("List", "clear", "List.clear()", "Removes every item."),
    member!("List", "insert", "List.insert(i: Int, item: T)", "Inserts item at index i."),
    member!("List", "remove", "List.remove(i: Int) -> T?", "Removes and returns the item at index i."),
    member!("List", "enumerate", "List.enumerate() -> [(idx: Int, item: T)]", "Pairs each item with its index."),
    member!("List", "zip", "List.zip(other: [U]) -> [(a: T, b: U)]", "Pairs items from two lists positionally."),
    member!("Map", "len", "Map.len() -> Int", "Number of entries."),
    member!("Map", "is_empty", "Map.is_empty() -> Bool", "True when there are no entries."),
    member!("Map", "get", "Map.get(key: K) -> V?", "Value for key, if present."),
    member!("Map", "add", "Map.add(key: K, value: V) -> V?", "Upserts and returns the displaced value."),
    member!("Map", "add_new", "Map.add_new(key: K, value: V) -> Bool", "Adds only when the key is absent."),
    member!("Map", "remove", "Map.remove(key: K) -> V?", "Removes and returns the value for key, if present."),
    member!("Map", "has_key", "Map.has_key(key: K) -> Bool", "True when key has an entry."),
    member!("Map", "keys", "Map.keys() -> [K]", "Every key, in map order."),
    member!("Map", "values", "Map.values() -> [V]", "Every value, in map order."),
    member!("Map", "each", "Map.each(f: fn(K, V))", "Runs f once per entry."),
    member!("String", "len", "String.len() -> Int", "Number of characters."),
    member!("String", "is_empty", "String.is_empty() -> Bool", "True when the string is empty."),
    member!("String", "contains", "String.contains(s: String) -> Bool", "True when s appears in the string."),
    member!("String", "starts_with", "String.starts_with(s: String) -> Bool", "True when the string starts with s."),
    member!("String", "ends_with", "String.ends_with(s: String) -> Bool", "True when the string ends with s."),
    member!("String", "trim", "String.trim() -> String", "Removes leading/trailing whitespace."),
    member!("String", "to_upper", "String.to_upper() -> String", "Uppercased copy."),
    member!("String", "to_lower", "String.to_lower() -> String", "Lowercased copy."),
    member!("String", "split", "String.split(sep: String) -> [String]", "Splits on every occurrence of sep."),
    member!("String", "lines", "String.lines() -> [String]", "Splits into lines."),
    member!("String", "chars", "String.chars() -> [Char]", "Every character, in order."),
    member!("String", "replace", "String.replace(from: String, to: String) -> String", "Replaces every occurrence of from with to."),
    member!("String", "repeat", "String.repeat(n: Int) -> String", "Concatenates n copies of the string."),
];

pub fn lookup(identity: &str) -> Option<&'static Symbol> {
    SYMBOLS.iter().find(|symbol| symbol.identity == identity)
}

pub fn members<'a>(owner: &'a str, prefix: &'a str) -> impl Iterator<Item = &'static Symbol> + 'a {
    SYMBOLS.iter().filter(move |symbol| symbol.owner == Some(owner) && symbol.member.starts_with(prefix))
}
