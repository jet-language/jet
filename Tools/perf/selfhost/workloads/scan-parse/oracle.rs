#[derive(Clone, Copy)]
struct Token {
    kind: &'static str,
    width: usize,
}

struct Node {
    kind: &'static str,
    children: usize,
}

fn parse(tokens: &[Token]) -> Vec<Node> {
    tokens
        .iter()
        .map(|token| Node {
            kind: token.kind,
            children: match token.kind {
                "keyword" => 1,
                "operator" => 2,
                _ => 0,
            },
        })
        .collect()
}

fn main() {
    let tokens = [
        Token { kind: "keyword", width: 2 },
        Token { kind: "identifier", width: 3 },
        Token { kind: "identifier", width: 4 },
        Token { kind: "identifier", width: 5 },
        Token { kind: "arrow", width: 2 },
        Token { kind: "identifier", width: 4 },
        Token { kind: "operator", width: 1 },
        Token { kind: "identifier", width: 5 },
    ];
    let nodes = parse(&tokens);
    let lexeme_bytes: usize = tokens.iter().map(|token| token.width).sum();
    let identifiers = tokens
        .iter()
        .filter(|token| token.kind == "identifier")
        .count();
    println!(
        "tokens={} lexeme_bytes={} identifiers={} ast_nodes={}",
        tokens.len(),
        lexeme_bytes,
        identifiers,
        nodes.len()
    );
    println!("parse=ok root={} ownership=owned", nodes[0].kind);
    let _children_sum: usize = nodes.iter().map(|node| node.children).sum();
}
