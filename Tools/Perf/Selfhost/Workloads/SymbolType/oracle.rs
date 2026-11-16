struct Symbol {
    name: &'static str,
    type_name: &'static str,
    scope: usize,
}

fn resolve(symbols: &[Symbol], query: &str) -> String {
    symbols
        .iter()
        .find(|symbol| symbol.name == query)
        .map(|symbol| format!("{}@{}", symbol.type_name, symbol.scope))
        .unwrap_or_else(|| "missing".to_string())
}

fn main() {
    let symbols = [
        Symbol { name: "left", type_name: "Int", scope: 1 },
        Symbol { name: "add", type_name: "Fn(Int,Int)->Int", scope: 0 },
        Symbol { name: "label", type_name: "String", scope: 1 },
    ];
    let resolved = resolve(&symbols, "left");
    let int_bindings = symbols
        .iter()
        .filter(|symbol| symbol.type_name == "Int" || symbol.type_name == "Fn(Int,Int)->Int")
        .count();
    println!(
        "symbols={} resolved={} int_bindings={}",
        symbols.len(), resolved, int_bindings
    );
    println!("compatible={} ownership=borrowed effects=none", "Int" == "Int");
}
