    // ── core.encoding.yaml: full std-only YAML 1.2 core → Dyn (c152) ─────────
    // D-ENC-YAML1 = A: block mappings + sequences (indentation-driven), flow `{}`/`[]`,
    // core-schema typed scalars (null/~, bool, int, float, str), single/double-quoted
    // + plain + block scalars (`|` literal, `>` folded with chomping), comments,
    // `---`/`...` document markers, anchors/aliases (`&a`/`*a`), binary and
    // timestamp tags. No external crates (I6).
    pub mod yaml {
        use super::Dyn;

        const MAX_YAML_DEPTH: usize = 64;
        const MAX_YAML_NODES: usize = 64 * 1024;
        const MAX_YAML_BYTES: usize = 64 * 1024 * 1024;
        const YAML_NODE_OVERHEAD: usize = 32;
        use std::collections::BTreeMap;

        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct ParseError {
            pub line: usize,
            pub message: String,
            pub unsupported: bool,
        }

        #[derive(Default)]
        struct YamlBudget {
            nodes: usize,
            bytes: usize,
        }

        impl YamlBudget {
            fn node(&mut self, payload: usize) -> Result<(), ()> {
                let nodes = self.nodes.checked_add(1).ok_or(())?;
                let cost = YAML_NODE_OVERHEAD.checked_add(payload).ok_or(())?;
                let bytes = self.bytes.checked_add(cost).ok_or(())?;
                if nodes > MAX_YAML_NODES || bytes > MAX_YAML_BYTES {
                    return Err(());
                }
                self.nodes = nodes;
                self.bytes = bytes;
                Ok(())
            }

            fn bytes(&mut self, amount: usize) -> Result<(), ()> {
                let bytes = self.bytes.checked_add(amount).ok_or(())?;
                if bytes > MAX_YAML_BYTES {
                    return Err(());
                }
                self.bytes = bytes;
                Ok(())
            }
        }

        enum FlowError {
            Depth,
            Budget,
            Syntax(String),
            Unsupported(String),
        }

        fn yaml_null(budget: &mut YamlBudget) -> Result<Dyn, ()> {
            budget.node(0)?;
            Ok(Dyn::Null)
        }

        fn yaml_scalar(s: &str, budget: &mut YamlBudget) -> Result<Dyn, FlowError> {
            let s = s.trim();
            let value = if let Some(text) = s.strip_prefix("!!binary ") {
                let text = match scalar_value(text) {
                    Dyn::Text(text) => text, _ => text.to_owned(),
                };
                Dyn::Bytes(jet_foundation::base_encoding_strict::decode_base64(&text, true, false)
                    .map_err(FlowError::Unsupported)?)
            } else if let Some(text) = s.strip_prefix("!!timestamp ") {
                let text = match scalar_value(text) {
                    Dyn::Text(text) => text, Dyn::DateTime(value) => value.to_rfc3339(),
                    _ => text.to_owned(),
                };
                Dyn::parse_datetime(&text).map_err(FlowError::Unsupported)?
            } else if s.is_ascii() && !s.starts_with(['\'', '"']) && s.len() >= 20
                && matches!(s.as_bytes().get(10), Some(b'T' | b't' | b' '))
                && (s.ends_with(['Z', 'z']) || s[19..].contains(['+', '-'])) {
                Dyn::parse_datetime(s).map_err(FlowError::Unsupported)?
            } else { scalar_value(s) };
            let payload = match &value {
                Dyn::Number(text) | Dyn::Text(text) => {
                    text.len()
                }
                Dyn::Bytes(bytes) => bytes.len(),
                _ => 0,
            };
            budget.node(payload).map_err(|_| FlowError::Budget)?;
            Ok(value)
        }

        fn yaml_clone(value: &Dyn, budget: &mut YamlBudget) -> Result<Dyn, ()> {
            match value {
                Dyn::Null | Dyn::Bool(_) | Dyn::Int(_) | Dyn::Float(_) | Dyn::DateTime(_) => {
                    budget.node(0)?;
                }
                Dyn::Number(text) | Dyn::Text(text) => {
                    budget.node(text.len())?;
                }
                Dyn::Bytes(bytes) => {
                    budget.node(bytes.len())?;
                }
                Dyn::Array(items) => {
                    budget.node(0)?;
                    for item in items {
                        yaml_clone(item, budget)?;
                    }
                }
                Dyn::Object(entries) => {
                    budget.node(0)?;
                    for (key, item) in entries {
                        budget.bytes(key.len())?;
                        yaml_clone(item, budget)?;
                    }
                }
            }
            Ok(value.clone())
        }

        fn yaml_flow_message(error: FlowError) -> String {
            match error {
                FlowError::Depth => "YAML value is nested too deeply".to_owned(),
                FlowError::Budget => "YAML value exceeds its node or byte budget".to_owned(),
                FlowError::Unsupported(message) | FlowError::Syntax(message) => message,
            }
        }

        pub fn parse_to_tree(raw: &str) -> Result<Dyn, ParseError> {
            let line_count = raw
                .as_bytes()
                .iter()
                .filter(|byte| **byte == b'\n')
                .count()
                .checked_add(1);
            if raw.len() > MAX_YAML_BYTES
                || !line_count.is_some_and(|count| count <= MAX_YAML_NODES)
            {
                return Err(ParseError {
                    line: 1,
                    message: "YAML input exceeds its byte or line budget".to_string(),
                    unsupported: false,
                });
            }
            let lines: Vec<String> = raw
                .split('\n')
                .map(|l| l.trim_end_matches('\r').to_string())
                .collect();
            let mut p = Parser {
                lines,
                pos: 0,
                anchors: BTreeMap::new(),
                budget: YamlBudget::default(),
            };
            p.skip_ignorable();
            // Leading document marker(s).
            while p.at_doc_marker() {
                p.pos = p.pos.saturating_add(1);
                p.skip_ignorable();
            }
            if p.pos >= p.lines.len() || p.at_doc_end() {
                return p.null();
            }
            let base = p.indent(p.pos);
            let value = p.parse_node(base, 0)?;
            p.skip_ignorable();
            if p.pos < p.lines.len() && !p.at_doc_marker() && !p.at_doc_end() {
                return Err(ParseError {
                    line: p.pos.saturating_add(1),
                    message: crate::jet_encoding_errors::YAML_EXPECTED_KEY_VALUE.into(),
                    unsupported: false,
                });
            }
            Ok(value)
        }

        struct Parser {
            lines: Vec<String>,
            pos: usize,
            anchors: BTreeMap<String, Dyn>,
            budget: YamlBudget,
        }

        impl Parser {
            fn indent(&self, i: usize) -> usize {
                self.lines[i].chars().take_while(|c| *c == ' ').count()
            }
            // The line's content with leading indent removed and any trailing comment
            // stripped (a ` #` outside quotes, or a leading `#`).
            fn content(&self, i: usize) -> String {
                let after = &self.lines[i][self.indent(i)..];
                strip_comment(after)
            }
            fn is_ignorable(&self, i: usize) -> bool {
                let t = self.lines[i].trim();
                t.is_empty() || t.starts_with('#')
            }
            fn skip_ignorable(&mut self) {
                while self.pos < self.lines.len() && self.is_ignorable(self.pos) {
                    self.pos = self.pos.saturating_add(1);
                }
            }
            fn at_doc_marker(&self) -> bool {
                self.pos < self.lines.len() && self.lines[self.pos].trim_start().starts_with("---")
            }
            fn at_doc_end(&self) -> bool {
                self.pos < self.lines.len() && self.lines[self.pos].trim() == "..."
            }

            fn limit_error(&self) -> ParseError {
                ParseError {
                    line: self.pos.saturating_add(1),
                    message: "YAML value exceeds its node or byte budget".to_string(),
                    unsupported: false,
                }
            }

            fn null(&mut self) -> Result<Dyn, ParseError> {
                yaml_null(&mut self.budget).map_err(|_| self.limit_error())
            }

            fn scalar(&mut self, value: &str) -> Result<Dyn, ParseError> {
                yaml_scalar(value, &mut self.budget).map_err(|error| ParseError {
                    line: self.pos.saturating_add(1),
                    unsupported: matches!(&error, FlowError::Unsupported(_)),
                    message: yaml_flow_message(error),
                })
            }

            fn array(&mut self, items: Vec<Dyn>) -> Result<Dyn, ParseError> {
                self.budget.node(0).map_err(|_| self.limit_error())?;
                Ok(Dyn::Array(items))
            }

            fn object(
                &mut self,
                entries: Vec<(String, Dyn)>,
            ) -> Result<Dyn, ParseError> {
                for (key, _) in &entries {
                    self.budget.bytes(key.len()).map_err(|_| self.limit_error())?;
                }
                self.budget.node(0).map_err(|_| self.limit_error())?;
                Ok(Dyn::Object(entries))
            }

            fn next_depth(&self, depth: usize) -> Result<usize, ParseError> {
                depth.checked_add(1).ok_or_else(|| self.limit_error())
            }

            fn next_indent(&self, indent: usize) -> Result<usize, ParseError> {
                indent.checked_add(1).ok_or_else(|| self.limit_error())
            }

            fn parse_node(&mut self, min_indent: usize, depth: usize) -> Result<Dyn, ParseError> {
                self.skip_ignorable();
                if self.pos >= self.lines.len() || self.at_doc_marker() || self.at_doc_end() {
                    return self.null();
                }
                if depth >= MAX_YAML_DEPTH {
                    return Err(ParseError {
                        line: self.pos.saturating_add(1),
                        message: "YAML value is nested too deeply".to_string(),
                        unsupported: false,
                    });
                }
                let ind = self.indent(self.pos);
                if ind < min_indent {
                    return self.null();
                }
                let content = self.content(self.pos);
                if content == "-" || content.starts_with("- ") {
                    self.parse_block_seq(ind, depth)
                } else if is_map_entry(&content) {
                    self.parse_block_map(ind, depth)
                } else {
                    // A bare scalar node (possibly anchored/aliased/flow/quoted).
                    self.pos = self.pos.saturating_add(1);
                    if content.starts_with("!!binary |") || content.starts_with("!!binary >") {
                        self.parse_block_scalar(ind, &content)
                    } else {
                        self.parse_inline_value(&content, depth)
                    }
                }
            }

            fn parse_block_seq(&mut self, indent: usize, depth: usize) -> Result<Dyn, ParseError> {
                let mut items = Vec::new();
                loop {
                    self.skip_ignorable();
                    if self.pos >= self.lines.len() || self.at_doc_marker() || self.at_doc_end() {
                        break;
                    }
                    let ind = self.indent(self.pos);
                    if ind != indent {
                        break;
                    }
                    let content = self.content(self.pos);
                    if content != "-" && !content.starts_with("- ") {
                        break;
                    }
                    // Dash trick: blank out `-` so the item content aligns as a normal
                    // block at indent+1, then parse it uniformly (scalar/map/seq).
                    let line = &mut self.lines[self.pos];
                    let bytes: Vec<char> = line.chars().collect();
                    // The dash sits at char index == indent (indentation is spaces only).
                    let mut rebuilt: String = bytes
                        .iter()
                        .enumerate()
                        .map(|(i, c)| if i == indent { ' ' } else { *c })
                        .collect();
                    // If nothing follows the dash, leave a blank line.
                    if rebuilt.trim().is_empty() {
                        rebuilt = String::new();
                    }
                    *line = rebuilt;
                    let item = self.parse_node(self.next_indent(indent)?, self.next_depth(depth)?)?;
                    items.push(item);
                }
                self.array(items)
            }

            fn parse_block_map(&mut self, indent: usize, depth: usize) -> Result<Dyn, ParseError> {
                let mut entries: Vec<(String, Dyn)> = Vec::new();
                loop {
                    self.skip_ignorable();
                    if self.pos >= self.lines.len() || self.at_doc_marker() || self.at_doc_end() {
                        break;
                    }
                    let ind = self.indent(self.pos);
                    if ind != indent {
                        break;
                    }
                    let content = self.content(self.pos);
                    if content.starts_with("- ") || content == "-" || !is_map_entry(&content) {
                        break;
                    }
                    let line_no = self.pos.saturating_add(1);
                    let (key, rest) = split_key(&content).ok_or_else(|| ParseError {
                        line: line_no,
                        message: crate::jet_encoding_errors::YAML_EXPECTED_KEY_VALUE.into(),
                        unsupported: false,
                    })?;
                    self.pos = self.pos.saturating_add(1);
                    let rest = rest.trim();
                    let value = if rest.is_empty() {
                        // Nested block (deeper indent) or empty → Null.
                        self.skip_ignorable();
                        if self.pos < self.lines.len()
                            && self.indent(self.pos) > indent
                            && !self.at_doc_marker()
                            && !self.at_doc_end()
                        {
                            self.parse_node(self.next_indent(indent)?, self.next_depth(depth)?)?
                        } else {
                            self.null()?
                        }
                    } else if rest.starts_with('|') || rest.starts_with('>')
                        || rest.starts_with("!!binary |") || rest.starts_with("!!binary >") {
                        self.parse_block_scalar(indent, rest)?
                    } else {
                        self.parse_inline_value(rest, self.next_depth(depth)?)?
                    };
                    if entries.iter().any(|(name, _)| name == &key) {
                        return Err(ParseError { line: line_no, message: format!("duplicate key `{key}`"), unsupported: false });
                    }
                    entries.push((key, value));
                }
                self.object(entries)
            }

            // A `|`/`>` block scalar. Following lines more-indented than the key form the
            // body; dedent by the first body line's indent. `>` folds line breaks to spaces.
            fn parse_block_scalar(
                &mut self,
                parent_indent: usize,
                header: &str,
            ) -> Result<Dyn, ParseError> {
                let binary = header.starts_with("!!binary ");
                let header = header.strip_prefix("!!binary ").unwrap_or(header);
                let folded = header.starts_with('>');
                let chomp = if header.contains('-') {
                    'S'
                } else if header.contains('+') {
                    'K'
                } else {
                    'C'
                };
                let mut body_lines: Vec<String> = Vec::new();
                let mut block_indent: Option<usize> = None;
                while self.pos < self.lines.len() {
                    let raw = &self.lines[self.pos];
                    if raw.trim().is_empty() {
                        body_lines.push(String::new());
                        self.pos = self.pos.saturating_add(1);
                        continue;
                    }
                    let ind = self.indent(self.pos);
                    if ind <= parent_indent {
                        break;
                    }
                    let bi = *block_indent.get_or_insert(ind);
                    let chars: Vec<char> = raw.chars().collect();
                    let start = bi.min(chars.len());
                    let dedented: String = chars[start..].iter().collect();
                    body_lines.push(dedented);
                    self.pos = self.pos.saturating_add(1);
                }
                // Drop trailing blank lines for chomping decisions.
                let mut text = if folded {
                    fold_lines(&body_lines)
                } else {
                    body_lines.join("\n")
                };
                let trimmed = text.trim_end_matches('\n').to_string();
                text = match chomp {
                    'S' => trimmed,                                        // strip: no trailing newline
                    'K' => text.trim_end_matches('\n').to_string() + "\n", // keep (simplified to one)
                    _ => trimmed + "\n", // clip: single trailing newline
                };
                self.budget
                    .node(text.len())
                    .map_err(|_| self.limit_error())?;
                if binary {
                    let bytes = jet_foundation::base_encoding_strict::decode_base64(&text, true, false)
                        .map_err(|message| ParseError { line: self.pos.saturating_add(1), message, unsupported: true })?;
                    Ok(Dyn::Bytes(bytes))
                } else { Ok(Dyn::Text(text)) }
            }

            fn parse_inline_value(&mut self, s: &str, depth: usize) -> Result<Dyn, ParseError> {
                let s = s.trim();
                // Anchor: `&name <value?>` — register the parsed value under `name`.
                if let Some(rest) = s.strip_prefix('&') {
                    let mut it = rest.splitn(2, char::is_whitespace);
                    let name = it.next().unwrap_or("").to_string();
                    self.budget
                        .bytes(name.len())
                        .map_err(|_| self.limit_error())?;
                    let val_str = it.next().unwrap_or("").trim();
                    let value = if val_str.is_empty() {
                        // The value is a nested block following this line.
                        self.parse_node(0, self.next_depth(depth)?)?
                    } else {
                        self.parse_inline_value(val_str, self.next_depth(depth)?)?
                    };
                    let stored = yaml_clone(&value, &mut self.budget)
                        .map_err(|_| self.limit_error())?;
                    self.anchors.insert(name, stored);
                    return Ok(value);
                }
                // Alias: `*name`.
                if let Some(name) = s.strip_prefix('*') {
                    if let Some(value) = self.anchors.get(name.trim()) {
                        return yaml_clone(value, &mut self.budget)
                            .map_err(|_| self.limit_error());
                    }
                    return self.null();
                }
                if s.starts_with('[') {
                    return parse_flow(s, depth, &mut self.budget)
                        .map(|(value, _)| value)
                        .map_err(|error| ParseError {
                            line: self.pos.saturating_add(1),
                            unsupported: matches!(&error, FlowError::Unsupported(_)),
                            message: yaml_flow_message(error),
                        });
                }
                if s.starts_with('{') {
                    return parse_flow(s, depth, &mut self.budget)
                        .map(|(value, _)| value)
                        .map_err(|error| ParseError {
                            line: self.pos.saturating_add(1),
                            unsupported: matches!(&error, FlowError::Unsupported(_)),
                            message: yaml_flow_message(error),
                        });
                }
                self.scalar(s)
            }
        }

        // ── Flow `[...]` / `{...}` (single-line) ─────────────────────────────────
        fn parse_flow(
            s: &str,
            depth: usize,
            budget: &mut YamlBudget,
        ) -> Result<(Dyn, usize), FlowError> {
            let chars: Vec<char> = s.chars().collect();
            parse_flow_at(&chars, 0, depth, budget)
        }
        fn parse_flow_at(
            chars: &[char],
            mut i: usize,
            depth: usize,
            budget: &mut YamlBudget,
        ) -> Result<(Dyn, usize), FlowError> {
            while i < chars.len() && chars[i].is_whitespace() {
                i = i.saturating_add(1);
            }
            if i >= chars.len() {
                return Ok((yaml_null(budget).map_err(|_| FlowError::Budget)?, i));
            }
            match chars[i] {
                '[' => {
                    if depth >= MAX_YAML_DEPTH {
                        return Err(FlowError::Depth);
                    }
                    i = i.saturating_add(1);
                    let mut items = Vec::new();
                    loop {
                        while i < chars.len() && (chars[i].is_whitespace() || chars[i] == ',') {
                            i = i.saturating_add(1);
                        }
                        if i >= chars.len() || chars[i] == ']' {
                            i = i.saturating_add(1);
                            break;
                        }
                        let next_depth = depth.checked_add(1).ok_or(FlowError::Depth)?;
                        let (v, ni) = parse_flow_at(chars, i, next_depth, budget)?;
                        items.push(v);
                        i = ni;
                        while i < chars.len() && chars[i].is_whitespace() {
                            i = i.saturating_add(1);
                        }
                        if i < chars.len() && chars[i] == ',' {
                            i = i.saturating_add(1);
                        } else if i < chars.len() && chars[i] == ']' {
                            i = i.saturating_add(1);
                            break;
                        }
                    }
                    budget.node(0).map_err(|_| FlowError::Budget)?;
                    Ok((Dyn::Array(items), i))
                }
                '{' => {
                    if depth >= MAX_YAML_DEPTH {
                        return Err(FlowError::Depth);
                    }
                    i = i.saturating_add(1);
                    let mut entries = Vec::new();
                    loop {
                        while i < chars.len() && (chars[i].is_whitespace() || chars[i] == ',') {
                            i = i.saturating_add(1);
                        }
                        if i >= chars.len() || chars[i] == '}' {
                            i = i.saturating_add(1);
                            break;
                        }
                        // key up to ':'
                        let (key, ni) = scan_flow_scalar(chars, i, true);
                        i = ni;
                        while i < chars.len() && chars[i].is_whitespace() {
                            i = i.saturating_add(1);
                        }
                        if i < chars.len() && chars[i] == ':' {
                            i = i.saturating_add(1);
                        }
                        let next_depth = depth.checked_add(1).ok_or(FlowError::Depth)?;
                        let (v, nj) = parse_flow_at(chars, i, next_depth, budget)?;
                        i = nj;
                        let key = unquote_key(key.trim());
                        if entries.iter().any(|(name, _)| name == &key) {
                            return Err(FlowError::Syntax(format!("duplicate key `{key}`")));
                        }
                        budget
                            .bytes(key.len())
                            .map_err(|_| FlowError::Budget)?;
                        entries.push((key, v));
                        while i < chars.len() && chars[i].is_whitespace() {
                            i = i.saturating_add(1);
                        }
                        if i < chars.len() && chars[i] == ',' {
                            i = i.saturating_add(1);
                        } else if i < chars.len() && chars[i] == '}' {
                            i = i.saturating_add(1);
                            break;
                        }
                    }
                    budget.node(0).map_err(|_| FlowError::Budget)?;
                    Ok((Dyn::Object(entries), i))
                }
                _ => {
                    let (raw, ni) = scan_flow_scalar(chars, i, false);
                    Ok((yaml_scalar(raw.trim(), budget)?, ni))
                }
            }
        }
        // Read a flow scalar (until `,`/`]`/`}`/`:` when key) honoring quotes.
        fn scan_flow_scalar(chars: &[char], mut i: usize, as_key: bool) -> (String, usize) {
            while i < chars.len() && chars[i].is_whitespace() {
                i = i.saturating_add(1);
            }
            if i < chars.len() && (chars[i] == '"' || chars[i] == '\'') {
                let q = chars[i];
                let mut out = String::new();
                i = i.saturating_add(1);
                while i < chars.len() {
                    if chars[i] == q {
                        if q == '\''
                            && i
                                .checked_add(1)
                                .is_some_and(|next| next < chars.len() && chars[next] == '\'')
                        {
                            out.push('\'');
                            i = i.saturating_add(2);
                            continue;
                        }
                        i = i.saturating_add(1);
                        break;
                    }
                    if chars[i] == '\\' && q == '"' {
                        if let Some(next) = i.checked_add(1).filter(|next| *next < chars.len()) {
                            out.push(unescape(chars[next]));
                            i = i.saturating_add(2);
                            continue;
                        }
                    }
                    out.push(chars[i]);
                    i = i.saturating_add(1);
                }
                return (out, i);
            }
            let mut out = String::new();
            while i < chars.len() {
                let c = chars[i];
                if c == ',' || c == ']' || c == '}' {
                    break;
                }
                if as_key && c == ':' {
                    break;
                }
                out.push(c);
                i = i.saturating_add(1);
            }
            (out, i)
        }

        fn fold_lines(lines: &[String]) -> String {
            // `>` folding: blank lines become newlines; consecutive non-blank lines join
            // with a single space.
            let mut out = String::new();
            let mut prev_blank = true;
            for l in lines {
                if l.trim().is_empty() {
                    out.push('\n');
                    prev_blank = true;
                } else {
                    if !prev_blank {
                        out.push(' ');
                    }
                    out.push_str(l);
                    prev_blank = false;
                }
            }
            out
        }

        fn unescape(c: char) -> char {
            match c {
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                '0' => '\0',
                '\\' => '\\',
                '"' => '"',
                _ => c,
            }
        }

        // Strip a trailing ` #...` comment that is outside quotes, or a leading `#`.
        fn strip_comment(s: &str) -> String {
            let chars: Vec<char> = s.chars().collect();
            let mut in_s = false;
            let mut in_d = false;
            let mut i = 0;
            while i < chars.len() {
                let c = chars[i];
                match c {
                    '\'' if !in_d => in_s = !in_s,
                    '"' if !in_s => in_d = !in_d,
                    '#' if !in_s
                        && !in_d
                        && (i == 0 || chars[i - 1] == ' ' || chars[i - 1] == '\t') =>
                    {
                        let kept: String = chars[..i].iter().collect();
                        return kept.trim_end().to_string();
                    }
                    _ => {}
                }
                i = i.saturating_add(1);
            }
            s.trim_end().to_string()
        }

        // Is this content a `key: value` mapping entry (top-level `:` outside flow/quotes)?
        fn is_map_entry(s: &str) -> bool {
            top_level_colon(s).is_some()
        }
        fn top_level_colon(s: &str) -> Option<usize> {
            let chars: Vec<char> = s.chars().collect();
            let mut in_s = false;
            let mut in_d = false;
            let mut depth = 0i32;
            let mut i = 0;
            while i < chars.len() {
                let c = chars[i];
                match c {
                    '\'' if !in_d => in_s = !in_s,
                    '"' if !in_s => in_d = !in_d,
                    '[' | '{' if !in_s && !in_d => depth = depth.saturating_add(1),
                    ']' | '}' if !in_s && !in_d => depth = depth.saturating_sub(1),
                    ':' if !in_s && !in_d && depth == 0 => {
                        // A mapping `:` must be followed by space or end-of-line.
                        if i
                            .checked_add(1)
                            .is_none_or(|next| next >= chars.len() || chars[next] == ' ')
                        {
                            return Some(i);
                        }
                    }
                    _ => {}
                }
                i = i.saturating_add(1);
            }
            None
        }
        fn split_key(s: &str) -> Option<(String, String)> {
            let idx = top_level_colon(s)?;
            let chars: Vec<char> = s.chars().collect();
            let key_raw: String = chars[..idx].iter().collect();
            let rest_start = idx.checked_add(1)?;
            let rest: String = chars[rest_start..].iter().collect();
            Some((unquote_key(key_raw.trim()), rest))
        }
        fn unquote_key(k: &str) -> String {
            if (k.starts_with('"') && k.ends_with('"') && k.len() >= 2)
                || (k.starts_with('\'') && k.ends_with('\'') && k.len() >= 2)
            {
                k[1..k.len() - 1].to_string()
            } else {
                k.to_string()
            }
        }

        // Type a plain/quoted scalar by the YAML core schema.
        fn scalar_value(s: &str) -> Dyn {
            let s = s.trim();
            if s.is_empty() {
                return Dyn::Null;
            }
            // Quoted strings are always text.
            if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
                let inner = &s[1..s.len() - 1];
                let mut out = String::new();
                let mut chars = inner.chars().peekable();
                while let Some(c) = chars.next() {
                    if c == '\\' {
                        if let Some(n) = chars.next() {
                            out.push(unescape(n));
                        }
                    } else {
                        out.push(c);
                    }
                }
                return Dyn::Text(out);
            }
            if s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2 {
                return Dyn::Text(s[1..s.len() - 1].replace("''", "'"));
            }
            if let Ok(value) = Dyn::parse_datetime(s) { return value; }
            match s {
                "null" | "Null" | "NULL" | "~" => return Dyn::Null,
                "true" | "True" | "TRUE" => return Dyn::Bool(true),
                "false" | "False" | "FALSE" => return Dyn::Bool(false),
                ".inf" | ".Inf" | ".INF" => return Dyn::Float(f64::INFINITY),
                "-.inf" | "-.Inf" => return Dyn::Float(f64::NEG_INFINITY),
                ".nan" | ".NaN" | ".NAN" => return Dyn::Float(f64::NAN),
                _ => {}
            }
            // #4602: a core-schema number keeps its exact lexeme as `Number`.
            if s.starts_with(|ch: char| ch.is_ascii_digit() || matches!(ch, '-' | '+' | '.')) {
                if let Some(lexeme) = crate::jet_json_number::number_lexeme(s) {
                    return Dyn::Number(lexeme);
                }
            }
            Dyn::Text(s.to_string())
        }

        // ── Render: a `Dyn` → block YAML text ───────────────────────────────
        pub fn render(t: &Dyn) -> String {
            let mut out = String::new();
            render_node(t, 0, &mut out);
            let s = out.trim_end().to_string();
            if s.is_empty() {
                "{}".to_string()
            } else {
                s
            }
        }
        fn render_node(t: &Dyn, indent: usize, out: &mut String) {
            let pad = " ".repeat(indent);
            match t {
                Dyn::Object(entries) => {
                    if entries.is_empty() {
                        out.push_str(&format!("{}{{}}\n", pad));
                        return;
                    }
                    for (k, v) in entries {
                        match v {
                            Dyn::Object(e) if !e.is_empty() => {
                                out.push_str(&format!("{}{}:\n", pad, render_key(k)));
                                render_node(v, indent + 2, out);
                            }
                            Dyn::Array(a) if !a.is_empty() => {
                                out.push_str(&format!("{}{}:\n", pad, render_key(k)));
                                render_seq(a, indent, out);
                            }
                            _ => {
                                out.push_str(&format!(
                                    "{}{}: {}\n",
                                    pad,
                                    render_key(k),
                                    render_scalar(v)
                                ));
                            }
                        }
                    }
                }
                Dyn::Array(items) => render_seq(items, indent, out),
                _ => out.push_str(&format!("{}{}\n", pad, render_scalar(t))),
            }
        }
        fn render_seq(items: &[Dyn], indent: usize, out: &mut String) {
            let pad = " ".repeat(indent);
            for item in items {
                match item {
                    Dyn::Object(e) if !e.is_empty() => {
                        out.push_str(&format!("{}-\n", pad));
                        render_node(item, indent + 2, out);
                    }
                    Dyn::Array(a) if !a.is_empty() => {
                        out.push_str(&format!("{}-\n", pad));
                        render_seq(a, indent + 2, out);
                    }
                    _ => out.push_str(&format!("{}- {}\n", pad, render_scalar(item))),
                }
            }
        }
        fn render_key(k: &str) -> String {
            if k.is_empty() || k.contains(':') || k.contains(' ') || k.contains('#') {
                format!("{:?}", k)
            } else {
                k.to_string()
            }
        }
        fn render_scalar(v: &Dyn) -> String {
            match v {
                Dyn::Null => "null".to_string(),
                Dyn::Bool(b) => b.to_string(),
                Dyn::Int(n) => n.to_string(),
                Dyn::Float(f) => format!("{:?}", f),
                Dyn::Number(text) => text.clone(),
                Dyn::Text(s) => {
                    if needs_quote(s) {
                        format!("{:?}", s)
                    } else {
                        s.clone()
                    }
                }
                Dyn::Bytes(bs) => format!("!!binary {}", Dyn::bytes_text(bs)),
                Dyn::DateTime(value) => value.to_rfc3339(),
                // An inline collection value renders in flow form.
                Dyn::Array(items) => {
                    let parts: Vec<String> = items.iter().map(render_scalar).collect();
                    format!("[{}]", parts.join(", "))
                }
                Dyn::Object(entries) => {
                    let parts: Vec<String> = entries
                        .iter()
                        .map(|(k, val)| format!("{}: {}", render_key(k), render_scalar(val)))
                        .collect();
                    format!("{{{}}}", parts.join(", "))
                }
            }
        }

        #[cfg(test)]
        mod tests {
            #[test]
            fn dyn_rules_reject_duplicate_keys_in_every_map_form() {
                for text in ["a: 1\na: 2", "{a: 1, a: 2}", "outer: {x: 1, x: 2}"] {
                    assert!(super::parse_to_tree(text).is_err(), "host YAML accepted {text}");
                }
            }

            #[test]
            fn dyn_rules_preserve_huge_number_lexemes() {
                let huge = "12345678901234567890123456789012345678901234567890";
                for token in [huge, "1e400"] {
                    let tree = super::parse_to_tree(&format!("a: {token}")).expect("host YAML number");
                    assert!(matches!(&tree, super::Dyn::Object(fields) if fields.iter().any(|(key, value)| key == "a" && matches!(value, super::Dyn::Number(text) if text == token))));
                }
            }
        }
        fn needs_quote(s: &str) -> bool {
            if s.is_empty() {
                return true;
            }
            matches!(
                s,
                "null"
                    | "Null"
                    | "NULL"
                    | "~"
                    | "true"
                    | "True"
                    | "TRUE"
                    | "false"
                    | "False"
                    | "FALSE"
            ) || s.parse::<i64>().is_ok()
                || s.parse::<f64>().is_ok()
                || (s.is_ascii() && s.len() >= 20
                    && matches!(s.as_bytes().get(10), Some(b'T' | b't' | b' '))
                    && (s.ends_with(['Z', 'z']) || s[19..].contains(['+', '-'])))
                || s.starts_with(' ')
                || s.ends_with(' ')
                || s.starts_with([
                    '-', '?', ':', '[', ']', '{', '}', '#', '&', '*', '!', '|', '>', '\'', '"',
                    '%', '@', '`',
                ])
                || s.contains(": ")
                || s.contains(" #")
                || s.contains('\n')
        }
    }
}
