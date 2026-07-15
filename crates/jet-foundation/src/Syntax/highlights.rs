/// D-HL1: generated editor grammars mark their owned sections with these
/// comments. Tests compare the committed section against fresh renderer output.
pub const HIGHLIGHT_GENERATED_START: &str = "BEGIN GENERATED JET SYNTAX HIGHLIGHTS";
pub const HIGHLIGHT_GENERATED_END: &str = "END GENERATED JET SYNTAX HIGHLIGHTS";

/// D-HL1: lexical highlight class for every token the generated grammars own.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum HighlightClass {
    KeywordControl,
    KeywordDeclaration,
    KeywordOwnership,
    KeywordOther,
    Literal,
    TypeBuiltin,
    Builtin,
    MarkerDirective,
    MarkerContract,
    Operator,
    Sigil,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct HighlightToken {
    pub text: &'static str,
    pub class: HighlightClass,
}

impl HighlightClass {
    pub fn textmate_scope(self) -> &'static str {
        match self {
            HighlightClass::KeywordControl => "keyword.control.jet",
            HighlightClass::KeywordDeclaration => "keyword.declaration.jet",
            HighlightClass::KeywordOwnership => "keyword.other.ownership.jet",
            HighlightClass::KeywordOther => "keyword.other.jet",
            HighlightClass::Literal => "constant.language.jet",
            HighlightClass::TypeBuiltin => "storage.type.builtin.jet",
            HighlightClass::Builtin => "support.function.builtin.jet",
            HighlightClass::MarkerDirective => "entity.name.tag.directive.jet",
            HighlightClass::MarkerContract => "entity.name.tag.contract.jet",
            HighlightClass::Operator => "keyword.operator.jet",
            HighlightClass::Sigil => "keyword.operator.sigil.jet",
        }
    }

    pub fn zed_capture(self) -> &'static str {
        match self {
            HighlightClass::KeywordControl => "@keyword.control",
            HighlightClass::KeywordDeclaration
            | HighlightClass::KeywordOwnership
            | HighlightClass::KeywordOther => "@keyword",
            HighlightClass::Literal => "@constant.builtin",
            HighlightClass::TypeBuiltin => "@type.builtin",
            HighlightClass::Builtin => "@function.builtin",
            HighlightClass::MarkerDirective | HighlightClass::MarkerContract => "@attribute",
            HighlightClass::Operator | HighlightClass::Sigil => "@operator",
        }
    }

    fn label(self) -> &'static str {
        match self {
            HighlightClass::KeywordControl => "keyword.control",
            HighlightClass::KeywordDeclaration => "keyword.declaration",
            HighlightClass::KeywordOwnership => "keyword.ownership",
            HighlightClass::KeywordOther => "keyword.other",
            HighlightClass::Literal => "literal",
            HighlightClass::TypeBuiltin => "type.builtin",
            HighlightClass::Builtin => "builtin",
            HighlightClass::MarkerDirective => "marker.directive",
            HighlightClass::MarkerContract => "marker.contract",
            HighlightClass::Operator => "operator",
            HighlightClass::Sigil => "sigil",
        }
    }
}

/// D-HL1: one source of truth for lexical editor highlighting. FOREIGN_* words
/// stay out; teaching diagnostics are not colored as live syntax.
pub const JET_HIGHLIGHT_TOKENS: &[HighlightToken] = &[
    // Control flow.
    HighlightToken {
        text: KW_IF,
        class: HighlightClass::KeywordControl,
    },
    HighlightToken {
        text: KW_ELSE,
        class: HighlightClass::KeywordControl,
    },
    HighlightToken {
        text: KW_LOOP,
        class: HighlightClass::KeywordControl,
    },
    HighlightToken {
        text: KW_IN,
        class: HighlightClass::KeywordControl,
    },
    HighlightToken {
        text: KW_BREAK,
        class: HighlightClass::KeywordControl,
    },
    HighlightToken {
        text: KW_CONTINUE,
        class: HighlightClass::KeywordControl,
    },
    HighlightToken {
        text: KW_RETURN,
        class: HighlightClass::KeywordControl,
    },
    HighlightToken {
        text: KW_RANGE_STEP,
        class: HighlightClass::KeywordControl,
    },
    // Declarations and contextual structure.
    HighlightToken {
        text: KW_FN,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_PUB,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_PRIV,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_USE,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_AS,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_EXTERN,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_RUST,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_MODULE,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_STRUCT,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_ENUM,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_ALIAS,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_IMPL,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_TRAIT,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_TAG,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_DERIVE,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_CONST,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_COMPTIME,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_DISTINCT,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_MIGRATION,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_RENAME,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_ADD,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_REMOVE,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_CHANGE,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_VIA,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_UNSAFE,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_IMPURE,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_REACTIVE,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_TASKGROUP,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: CTX_BLOCK,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_TRANSACT,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_TEST,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_BENCH,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_PURE,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_TODO,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_TAINTED,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_SANITIZER,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_STATE,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_TRANSITION,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_STATE_DECL,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_PROTOCOL,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: PROTO_CLIENT,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: PROTO_SERVER,
        class: HighlightClass::KeywordDeclaration,
    },
    HighlightToken {
        text: KW_VALIDATE_BLOCK,
        class: HighlightClass::KeywordDeclaration,
    },
    // Ownership / builtins.
    HighlightToken {
        text: KW_SELF,
        class: HighlightClass::KeywordOther,
    },
    HighlightToken {
        text: KW_UNINIT,
        class: HighlightClass::KeywordOwnership,
    },
    HighlightToken {
        text: KW_IT,
        class: HighlightClass::KeywordOther,
    },
    HighlightToken {
        text: BUILTIN_PRINT,
        class: HighlightClass::Builtin,
    },
    HighlightToken {
        text: BUILTIN_INPUT,
        class: HighlightClass::Builtin,
    },
    HighlightToken {
        text: VALIDATE_CHECK_FN,
        class: HighlightClass::Builtin,
    },
    // Literals.
    HighlightToken {
        text: LIT_TRUE,
        class: HighlightClass::Literal,
    },
    HighlightToken {
        text: LIT_FALSE,
        class: HighlightClass::Literal,
    },
    HighlightToken {
        text: LIT_NULL,
        class: HighlightClass::Literal,
    },
    HighlightToken {
        text: LIT_VALUE,
        class: HighlightClass::Literal,
    },
    HighlightToken {
        text: LIT_OK,
        class: HighlightClass::Literal,
    },
    HighlightToken {
        text: LIT_ERR,
        class: HighlightClass::Literal,
    },
    // Built-in types.
    HighlightToken {
        text: TYPE_INT,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_FLOAT,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_BOOL,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_STRING,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_ERROR,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_VOID,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_CHAR,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_SHARED,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_BUDGET,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_BUDGET_APPLIES,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_HASH_MAP,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_BTREE_MAP,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_DEQUE,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_SET,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_SORTED_SET,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_PRIORITY_QUEUE,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_LRU,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_BIT_SET,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_BYTE_BUFFER,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_STREAM,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_TASKGROUP,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_SELECT_BUILDER,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_SIGNAL,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_DERIVED,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_COMPUTED,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_EVENT,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_HOOK,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_SUBSCRIPTION,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_EVENT_SCOPE,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_EVENT_POLICY,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_EVENT_TRACE,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_WATCH_HANDLE,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_WATCH_SET,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_WATCH_EVENT,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_EFFECT,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_MEASUREMENT,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_PTR,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_BIGINT,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_DECIMAL,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_KEY,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_IO_ERROR,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_UTF8_ERROR,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_JSON,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_JSON_ERROR,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_DATA,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_DATA_JSON,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_DATA_TOML,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_DATA_YAML,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_DATA_CSV,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_DB_VALUE,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_I8,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_I16,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_I32,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_I64,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_U8,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_U16,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_U32,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_U64,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_F32,
        class: HighlightClass::TypeBuiltin,
    },
    HighlightToken {
        text: TYPE_F64,
        class: HighlightClass::TypeBuiltin,
    },
    // Operators and sigils.
    HighlightToken {
        text: SIGIL_BIND_IMMUT,
        class: HighlightClass::Sigil,
    },
    HighlightToken {
        text: SIGIL_BIND_MUT,
        class: HighlightClass::Sigil,
    },
    HighlightToken {
        text: SIGIL_MOVE,
        class: HighlightClass::Sigil,
    },
    HighlightToken {
        text: SIGIL_WRITE,
        class: HighlightClass::Sigil,
    },
    HighlightToken {
        text: SIGIL_COPY,
        class: HighlightClass::Sigil,
    },
    HighlightToken {
        text: SIGIL_SPREAD,
        class: HighlightClass::Sigil,
    },
    HighlightToken {
        text: OP_TRY_SUFFIX,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_RANGE,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_ARM_ARROW,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_LAMBDA_ARROW,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_PLUS,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_MINUS,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_STAR,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_SLASH,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_PERCENT,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_PIPE,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_SHL,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_SHR,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_AND,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_OR,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_NOT,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_EQ,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_NE,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_LT,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_GT,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_LE,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_GE,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_PLUS_EQ,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_PLUS_PLUS,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_MINUS_EQ,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_MINUS_MINUS,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_STAR_EQ,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_SLASH_EQ,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_PERCENT_EQ,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_AMP_EQ,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_PIPE_EQ,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_CARET_EQ,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_SHL_EQ,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_SHR_EQ,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_FALLBACK,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_OPTIONAL_CHAIN,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_NAMED_CTOR,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: OP_FAN_OUT,
        class: HighlightClass::Operator,
    },
    HighlightToken {
        text: TYPE_FIXED_SIZE_SEP,
        class: HighlightClass::Sigil,
    },
    HighlightToken {
        text: ATTR_PREFIX,
        class: HighlightClass::Sigil,
    },
    HighlightToken {
        text: CONTRACT_PREFIX,
        class: HighlightClass::Sigil,
    },
];

pub fn highlighted_tokens_sorted() -> Vec<HighlightToken> {
    let mut tokens = JET_HIGHLIGHT_TOKENS.to_vec();
    for &marker in DIRECTIVE_MARKERS {
        tokens.push(HighlightToken {
            text: marker,
            class: HighlightClass::MarkerDirective,
        });
    }
    for &marker in CONTRACT_MARKERS {
        tokens.push(HighlightToken {
            text: marker,
            class: HighlightClass::MarkerContract,
        });
    }
    tokens.sort_by(|a, b| a.class.cmp(&b.class).then(a.text.cmp(b.text)));
    tokens.dedup_by(|a, b| a.text == b.text && a.class == b.class);
    tokens
}

pub fn render_vscode_generated_highlights() -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "      \"comment\": \"{}\",\n",
        HIGHLIGHT_GENERATED_START
    ));
    out.push_str("      \"patterns\": [\n");
    let classes = [
        HighlightClass::KeywordControl,
        HighlightClass::KeywordDeclaration,
        HighlightClass::KeywordOwnership,
        HighlightClass::KeywordOther,
        HighlightClass::Literal,
        HighlightClass::TypeBuiltin,
        HighlightClass::Builtin,
        HighlightClass::MarkerDirective,
        HighlightClass::MarkerContract,
        HighlightClass::Sigil,
        HighlightClass::Operator,
    ];
    let mut first = true;
    for class in classes {
        let words = class_words(class);
        let symbols = class_symbols(class);
        if !words.is_empty() {
            push_vscode_pattern(
                &mut out,
                &mut first,
                class.textmate_scope(),
                &format!("\\b({})\\b", words.join("|")),
            );
        }
        if !symbols.is_empty() {
            push_vscode_pattern(
                &mut out,
                &mut first,
                class.textmate_scope(),
                &format!("({})", symbols.join("|")),
            );
        }
    }
    out.push_str("\n      ],\n");
    out.push_str(&format!(
        "      \"endComment\": \"{}\"\n",
        HIGHLIGHT_GENERATED_END
    ));
    out
}

pub fn render_tree_sitter_generated_highlights() -> String {
    let mut out = String::new();
    out.push_str(&format!("// {}\n", HIGHLIGHT_GENERATED_START));
    for class in [
        HighlightClass::KeywordControl,
        HighlightClass::KeywordDeclaration,
        HighlightClass::KeywordOwnership,
        HighlightClass::KeywordOther,
        HighlightClass::Literal,
        HighlightClass::TypeBuiltin,
        HighlightClass::Builtin,
        HighlightClass::MarkerDirective,
        HighlightClass::MarkerContract,
        HighlightClass::Sigil,
        HighlightClass::Operator,
    ] {
        let values = class_texts(class);
        out.push_str(&format!(
            "const {} = [{}];\n",
            tree_sitter_const_name(class),
            values
                .iter()
                .map(|s| format!("{:?}", s))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    out.push_str(&format!("// {}\n", HIGHLIGHT_GENERATED_END));
    out
}

pub fn render_zed_generated_highlights() -> String {
    let mut out = String::new();
    out.push_str(&format!("; {}\n", HIGHLIGHT_GENERATED_START));
    for class in [
        HighlightClass::KeywordControl,
        HighlightClass::KeywordDeclaration,
        HighlightClass::KeywordOwnership,
        HighlightClass::KeywordOther,
        HighlightClass::Literal,
        HighlightClass::TypeBuiltin,
        HighlightClass::Builtin,
        HighlightClass::MarkerDirective,
        HighlightClass::MarkerContract,
        HighlightClass::Sigil,
        HighlightClass::Operator,
    ] {
        let values = class_texts(class);
        out.push_str(&format!("; {}: {}\n", class.label(), values.join(" ")));
        if class == HighlightClass::MarkerDirective || class == HighlightClass::MarkerContract {
            continue;
        }
        let query_words = values
            .iter()
            .filter(|s| is_word_token(s) && is_zed_anonymous_word_token(s))
            .map(|s| format!("  {:?}", s))
            .collect::<Vec<_>>();
        if !query_words.is_empty() {
            out.push_str("[\n");
            out.push_str(&query_words.join("\n"));
            out.push_str(&format!("\n] {}\n\n", class.zed_capture()));
        }
    }
    out.push_str(&format!("; {}\n", HIGHLIGHT_GENERATED_END));
    out
}

fn push_vscode_pattern(out: &mut String, first: &mut bool, scope: &str, pattern: &str) {
    if !*first {
        out.push_str(",\n");
    }
    *first = false;
    out.push_str("        {\n");
    out.push_str(&format!("          \"name\": \"{}\",\n", scope));
    out.push_str(&format!(
        "          \"match\": \"{}\"\n",
        json_escape(pattern)
    ));
    out.push_str("        }");
}

fn class_texts(class: HighlightClass) -> Vec<&'static str> {
    let mut values = highlighted_tokens_sorted()
        .into_iter()
        .filter(|token| token.class == class)
        .map(|token| token.text)
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

fn class_words(class: HighlightClass) -> Vec<String> {
    class_texts(class)
        .into_iter()
        .filter(|s| is_word_token(s))
        .map(regex_escape)
        .collect()
}

fn class_symbols(class: HighlightClass) -> Vec<String> {
    class_texts(class)
        .into_iter()
        .filter(|s| !is_word_token(s))
        .map(regex_escape)
        .collect()
}

fn is_word_token(s: &str) -> bool {
    s.chars().all(|c| c == '_' || c.is_ascii_alphanumeric())
        && s.chars()
            .next()
            .is_some_and(|c| c == '_' || c.is_ascii_alphabetic())
}

fn is_zed_anonymous_word_token(s: &str) -> bool {
    // Zed validates query string literals against anonymous tree-sitter tokens.
    // Many generated highlight words are parsed as named nodes instead
    // (`type_identifier`, `marker_name`, `identifier`, etc.); emitting them here
    // makes the whole Jet language fail to load.
    matches!(
        s,
        "Bench"
            | "Bool"
            | "Char"
            | "Error"
            | "F32"
            | "F64"
            | "Float"
            | "I16"
            | "I32"
            | "I64"
            | "I8"
            | "Int"
            | "List"
            | "Map"
            | "String"
            | "Test"
            | "U16"
            | "U32"
            | "U64"
            | "U8"
            | "Void"
            | "add"
            | "as"
            | "break"
            | "change"
            | "comptime"
            | "const"
            | "continue"
            | "copy"
            | "derive"
            | "distinct"
            | "else"
            | "enum"
            | "err"
            | "extern"
            | "false"
            | "fn"
            | "if"
            | "impl"
            | "in"
            | "loop"
            | "migration"
            | "module"
            | "ok"
            | "pub"
            | "remove"
            | "rename"
            | "return"
            | "rust"
            | "self"
            | "step"
            | "struct"
            | "tag"
            | "trait"
            | "true"
            | "use"
            | "via"
    )
}

fn regex_escape(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        if matches!(
            ch,
            '\\' | '.'
                | '+'
                | '*'
                | '?'
                | '^'
                | '$'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | '|'
                | '/'
                | '-'
        ) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn tree_sitter_const_name(class: HighlightClass) -> &'static str {
    match class {
        HighlightClass::KeywordControl => "JET_HIGHLIGHT_KEYWORD_CONTROL",
        HighlightClass::KeywordDeclaration => "JET_HIGHLIGHT_KEYWORD_DECLARATION",
        HighlightClass::KeywordOwnership => "JET_HIGHLIGHT_KEYWORD_OWNERSHIP",
        HighlightClass::KeywordOther => "JET_HIGHLIGHT_KEYWORD_OTHER",
        HighlightClass::Literal => "JET_HIGHLIGHT_LITERAL",
        HighlightClass::TypeBuiltin => "JET_HIGHLIGHT_TYPE_BUILTIN",
        HighlightClass::Builtin => "JET_HIGHLIGHT_BUILTIN",
        HighlightClass::MarkerDirective => "JET_HIGHLIGHT_MARKER_DIRECTIVE",
        HighlightClass::MarkerContract => "JET_HIGHLIGHT_MARKER_CONTRACT",
        HighlightClass::Operator => "JET_HIGHLIGHT_OPERATOR",
        HighlightClass::Sigil => "JET_HIGHLIGHT_SIGIL",
    }
}
use super::{
    ATTR_PREFIX, BUILTIN_INPUT, BUILTIN_PRINT, CONTRACT_MARKERS, CONTRACT_PREFIX, CTX_BLOCK,
    DIRECTIVE_MARKERS, KW_ADD, KW_ALIAS, KW_AS, KW_BENCH, KW_BREAK, KW_CHANGE,
    KW_COMPTIME, KW_CONST, KW_CONTINUE, KW_DERIVE, KW_DISTINCT, KW_ELSE, KW_ENUM,
    KW_EXTERN, KW_FN, KW_IF, KW_IMPL, KW_IMPURE, KW_IN, KW_IT, KW_LOOP,
    KW_MIGRATION, KW_MODULE, KW_PRIV, KW_PROTOCOL, KW_PUB, KW_PURE, KW_RANGE_STEP,
    KW_REACTIVE, KW_REMOVE, KW_RENAME, KW_RETURN, KW_RUST, KW_SANITIZER, KW_SELF,
    KW_STATE, KW_STATE_DECL, KW_STRUCT, KW_TAG, KW_TAINTED, KW_TASKGROUP, KW_TEST, KW_TODO,
    KW_TRAIT, KW_TRANSACT, KW_TRANSITION, KW_UNINIT, KW_UNSAFE, KW_USE, KW_VALIDATE_BLOCK,
    KW_VIA, LIT_ERR, VALIDATE_CHECK_FN,
    LIT_FALSE, LIT_NULL, LIT_OK, LIT_TRUE, LIT_VALUE, OP_AMP_EQ, OP_AND, OP_ARM_ARROW,
    OP_CARET_EQ, OP_EQ, OP_FALLBACK, OP_FAN_OUT, OP_GE, OP_GT, OP_LAMBDA_ARROW, OP_LE, OP_LT,
    OP_MINUS, OP_MINUS_EQ, OP_MINUS_MINUS, OP_NAMED_CTOR, OP_NE, OP_NOT, OP_OPTIONAL_CHAIN,
    OP_OR, OP_PERCENT, OP_PERCENT_EQ, OP_PIPE, OP_PIPE_EQ, OP_PLUS, OP_PLUS_EQ, OP_PLUS_PLUS,
    OP_RANGE, OP_SHL, OP_SHL_EQ, OP_SHR, OP_SHR_EQ, OP_SLASH, OP_SLASH_EQ, OP_STAR,
    OP_STAR_EQ, OP_TRY_SUFFIX, PROTO_CLIENT, PROTO_SERVER, SIGIL_BIND_IMMUT, SIGIL_BIND_MUT,
    SIGIL_COPY, SIGIL_MOVE, SIGIL_SPREAD, SIGIL_WRITE, TYPE_BIGINT, TYPE_BIT_SET, TYPE_BOOL, TYPE_BTREE_MAP,
    TYPE_BUDGET, TYPE_BUDGET_APPLIES,
    TYPE_BYTE_BUFFER, TYPE_CHAR, TYPE_COMPUTED, TYPE_DATA, TYPE_DATA_CSV, TYPE_DATA_JSON,
    TYPE_DATA_TOML, TYPE_DATA_YAML, TYPE_DB_VALUE, TYPE_DECIMAL, TYPE_DEQUE, TYPE_DERIVED,
    TYPE_EFFECT, TYPE_ERROR, TYPE_EVENT, TYPE_EVENT_POLICY, TYPE_EVENT_SCOPE, TYPE_EVENT_TRACE,
    TYPE_F32, TYPE_F64, TYPE_FIXED_SIZE_SEP, TYPE_FLOAT, TYPE_HASH_MAP, TYPE_HOOK, TYPE_I16,
    TYPE_I32, TYPE_I64, TYPE_I8, TYPE_INT, TYPE_IO_ERROR, TYPE_JSON, TYPE_JSON_ERROR, TYPE_KEY,
    TYPE_LRU, TYPE_MEASUREMENT, TYPE_PRIORITY_QUEUE, TYPE_PTR, TYPE_SELECT_BUILDER, TYPE_SET,
    TYPE_SHARED, TYPE_SIGNAL, TYPE_SORTED_SET, TYPE_STREAM, TYPE_STRING, TYPE_SUBSCRIPTION,
    TYPE_TASKGROUP, TYPE_U16, TYPE_U32, TYPE_U64, TYPE_U8, TYPE_UTF8_ERROR, TYPE_VOID,
    TYPE_WATCH_EVENT, TYPE_WATCH_HANDLE, TYPE_WATCH_SET,
};
use crate::JSON::json_escape;
