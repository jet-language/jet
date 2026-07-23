//! D-PROTO1 / D-PROTO2: protocol/session type expansion.
//!
//! A `protocol Name { … }` block declares an ordered message exchange. Sema expands
//! it into `#SingleUse` `.Client`/`.Server` handle structs plus `state`/`#Transition`
//! impl methods, then re-parses the fragments through the normal front end (R11).

use crate::Diagnostics::Diagnostic;
use crate::Syntax;
use crate::AST::{Item, ProtocolDecl, ProtocolDirection, ProtocolMessage};

/// Expand every `protocol` declaration in `items`, replacing each with its generated
/// handle types and methods. Parse/lex failures are recorded in `diags`.
pub fn expand_module_protocols(items: &mut Vec<Item>, diags: &mut Vec<Diagnostic>) {
    let mut i = 0;
    while i < items.len() {
        let decl = match &items[i] {
            Item::ProtocolDecl(d) => d.clone(),
            _ => {
                i += 1;
                continue;
            }
        };
        items.remove(i);
        let fragment = generate_protocol_source(&decl);
        match lex_parse_fragment(&fragment, diags) {
            Ok(parsed) => {
                for item in parsed {
                    items.insert(i, item);
                    i += 1;
                }
            }
            Err(()) => {
                diags.push(Diagnostic::error(
                    "E0153",
                    format!(
                        "protocol `{}` failed to expand into handle types",
                        decl.name
                    ),
                    "the compiler generates `#SingleUse` `.Client`/`.Server` stubs from \
                     the protocol block — a fragment did not parse"
                        .to_string(),
                    "check the protocol declaration for typos; if this persists, file a bug"
                        .to_string(),
                    Some(decl.name_span),
                ));
            }
        }
    }
}

fn lex_parse_fragment(src: &str, diags: &mut Vec<Diagnostic>) -> Result<Vec<Item>, ()> {
    let (toks, lex_diags) = crate::Lexer::lex(src);
    if !lex_diags.is_empty() {
        diags.extend(lex_diags);
        return Err(());
    }
    match crate::Parser::parse(&toks) {
        Ok(prog) => Ok(prog.items),
        Err(parse_diags) => {
            diags.extend(parse_diags);
            Err(())
        }
    }
}

fn generate_protocol_source(decl: &ProtocolDecl) -> String {
    let mut out = String::new();
    let client = format!("{}.Client", decl.name);
    let server = format!("{}.Server", decl.name);
    let state_count = decl.messages.len() + 1;
    let states: Vec<String> = (0..state_count).map(|i| format!("S{i}")).collect();
    let state_list = states.join(", ");

    out.push_str(&format!("state {client} {{ {state_list} }}\n"));
    out.push_str(&format!("state {server} {{ {state_list} }}\n\n"));
    out.push_str(&format!(
        "#{} struct {client} {{\n    _token: Int,\n}}\n\n",
        Syntax::ATTR_SINGLE_USE
    ));
    out.push_str(&format!(
        "#{} struct {server} {{\n    _token: Int,\n}}\n\n",
        Syntax::ATTR_SINGLE_USE
    ));

    out.push_str(&format!("impl {client} {{\n"));
    out.push_str(&format!(
        "    #{}(_ -> S0) fn client() -> {client} {{\n        return {client}.{{ _token: 0 }}\n    }}\n\n",
        Syntax::KW_TRANSITION
    ));
    for (idx, msg) in decl.messages.iter().enumerate() {
        let terminal = idx + 1 == decl.messages.len();
        if matches!(msg.direction, ProtocolDirection::ClientToServer) {
            append_send_method(&mut out, &client, &states, idx, msg, terminal);
        } else {
            append_recv_method(&mut out, &client, &states, idx, msg, terminal);
        }
    }
    out.push_str("}\n\n");

    out.push_str(&format!("impl {server} {{\n"));
    out.push_str(&format!(
        "    #{}(_ -> S0) fn server() -> {server} {{\n        return {server}.{{ _token: 0 }}\n    }}\n\n",
        Syntax::KW_TRANSITION
    ));
    for (idx, msg) in decl.messages.iter().enumerate() {
        let terminal = idx + 1 == decl.messages.len();
        if matches!(msg.direction, ProtocolDirection::ClientToServer) {
            append_recv_method(&mut out, &server, &states, idx, msg, terminal);
        } else {
            append_send_method(&mut out, &server, &states, idx, msg, terminal);
        }
    }
    out.push('}');
    out
}

fn append_send_method(
    out: &mut String,
    handle: &str,
    states: &[String],
    idx: usize,
    msg: &ProtocolMessage,
    terminal: bool,
) {
    let from = &states[idx];
    let to = &states[idx + 1];
    let fields = format_fields(msg);
    let param_suffix = if fields.is_empty() {
        String::new()
    } else {
        format!(", {fields}")
    };
    if terminal {
        out.push_str(&format!(
            "    #{}({from} -> {to}) fn {}(self: ^{handle}{param_suffix}) {{\n        return\n    }}\n\n",
            Syntax::KW_TRANSITION,
            msg.name,
        ));
    } else {
        out.push_str(&format!(
            "    #{}({from} -> {to}) fn {}(self: ^{handle}{param_suffix}) -> {handle} ? Error {{\n        return Ok(self)\n    }}\n\n",
            Syntax::KW_TRANSITION,
            msg.name,
        ));
    }
}

fn append_recv_method(
    out: &mut String,
    handle: &str,
    states: &[String],
    idx: usize,
    msg: &ProtocolMessage,
    terminal: bool,
) {
    let from = &states[idx];
    let to = &states[idx + 1];
    let method = format!("recv_{}", msg.name);
    if terminal {
        out.push_str(&format!(
            "    #{}({from} -> {to}) fn {method}(self: ^{handle}) {{\n        return\n    }}\n\n",
            Syntax::KW_TRANSITION,
        ));
    } else {
        out.push_str(&format!(
            "    #{}({from} -> {to}) fn {method}(self: ^{handle}) -> {handle} ? Error {{\n        return Ok(self)\n    }}\n\n",
            Syntax::KW_TRANSITION,
        ));
    }
}

fn format_fields(msg: &ProtocolMessage) -> String {
    msg.fields
        .iter()
        .map(|(name, ty)| format!("{name}: {}", ty.name()))
        .collect::<Vec<_>>()
        .join(", ")
}
