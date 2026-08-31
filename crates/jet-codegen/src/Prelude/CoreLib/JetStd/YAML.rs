    // ── core.encoding.yaml: full std-only YAML 1.2 core → DataTree (c152) ─────────
    // D-ENC-YAML1 = A: block mappings + sequences (indentation-driven), flow `{}`/`[]`,
    // core-schema typed scalars (null/~, bool, int, float, str), single/double-quoted
    // + plain + block scalars (`|` literal, `>` folded with chomping), comments,
    // `---`/`...` document markers, and anchors/aliases (`&a`/`*a`). Explicit/custom
    // tags (`!!str`, `!MyType`) are deferred to c153 (frozen). No external crates (I6).
    pub mod yaml {
        use super::DataTree;

        const MAX_YAML_DEPTH: usize = 64;
        const MAX_YAML_NODES: usize = 64 * 1024;
        const MAX_YAML_BYTES: usize = 64 * 1024 * 1024;
        const YAML_NODE_OVERHEAD: usize = 32;
        use std::collections::BTreeMap;

        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct ParseError {
            pub line: usize,
            pub message: String,
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
        }

        fn yaml_null(budget: &mut YamlBudget) -> Result<DataTree, ()> {
            budget.node(0)?;
            Ok(DataTree::Null)
        }

        fn yaml_scalar(s: &str, budget: &mut YamlBudget) -> Result<DataTree, ()> {
            let value = scalar_value(s);
            let payload = match &value {
                DataTree::Number(text) | DataTree::TypedText(text) | DataTree::Text(text) => {
                    text.len()
                }
                DataTree::Bytes(bytes) => bytes.len(),
                _ => 0,
            };
            budget.node(payload)?;
            Ok(value)
        }

        fn yaml_clone(value: &DataTree, budget: &mut YamlBudget) -> Result<DataTree, ()> {
            match value {
                DataTree::Null | DataTree::Bool(_) | DataTree::Int(_) | DataTree::Float(_) => {
                    budget.node(0)?;
                }
                DataTree::Number(text) | DataTree::TypedText(text) | DataTree::Text(text) => {
                    budget.node(text.len())?;
                }
                DataTree::Bytes(bytes) => {
                    budget.node(bytes.len())?;
                }
                DataTree::Array(items) => {
                    budget.node(0)?;
                    for item in items {
                        yaml_clone(item, budget)?;
                    }
                }
                DataTree::Object(entries) => {
                    budget.node(0)?;
                    for (key, item) in entries {
                        budget.bytes(key.len())?;
                        yaml_clone(item, budget)?;
                    }
                }
            }
            Ok(value.clone())
        }

        fn yaml_flow_message(error: FlowError) -> &'static str {
            match error {
                FlowError::Depth => "YAML value is nested too deeply",
                FlowError::Budget => "YAML value exceeds its node or byte budget",
            }
        }

        pub fn parse_to_tree(raw: &str) -> Result<DataTree, ParseError> {
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
                });
            }
            Ok(value)
        }

        struct Parser {
            lines: Vec<String>,
            pos: usize,
            anchors: BTreeMap<String, DataTree>,
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
                }
            }

            fn null(&mut self) -> Result<DataTree, ParseError> {
                yaml_null(&mut self.budget).map_err(|_| self.limit_error())
            }

            fn scalar(&mut self, value: &str) -> Result<DataTree, ParseError> {
                yaml_scalar(value, &mut self.budget).map_err(|_| self.limit_error())
            }

            fn array(&mut self, items: Vec<DataTree>) -> Result<DataTree, ParseError> {
                self.budget.node(0).map_err(|_| self.limit_error())?;
                Ok(DataTree::Array(items))
            }

            fn object(
                &mut self,
                entries: Vec<(String, DataTree)>,
            ) -> Result<DataTree, ParseError> {
                for (key, _) in &entries {
                    self.budget.bytes(key.len()).map_err(|_| self.limit_error())?;
                }
                self.budget.node(0).map_err(|_| self.limit_error())?;
                Ok(DataTree::Object(entries))
            }

            fn next_depth(&self, depth: usize) -> Result<usize, ParseError> {
                depth.checked_add(1).ok_or_else(|| self.limit_error())
            }

            fn next_indent(&self, indent: usize) -> Result<usize, ParseError> {
                indent.checked_add(1).ok_or_else(|| self.limit_error())
            }

            fn parse_node(&mut self, min_indent: usize, depth: usize) -> Result<DataTree, ParseError> {
                self.skip_ignorable();
                if self.pos >= self.lines.len() || self.at_doc_marker() || self.at_doc_end() {
                    return self.null();
                }
                if depth >= MAX_YAML_DEPTH {
                    return Err(ParseError {
                        line: self.pos.saturating_add(1),
                        message: "YAML value is nested too deeply".to_string(),
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
                    self.parse_inline_value(&content, depth)
                }
            }

            fn parse_block_seq(&mut self, indent: usize, depth: usize) -> Result<DataTree, ParseError> {
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

            fn parse_block_map(&mut self, indent: usize, depth: usize) -> Result<DataTree, ParseError> {
                let mut entries: Vec<(String, DataTree)> = Vec::new();
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
                    } else if rest.starts_with('|') || rest.starts_with('>') {
                        self.parse_block_scalar(indent, rest)?
                    } else {
                        self.parse_inline_value(rest, self.next_depth(depth)?)?
                    };
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
            ) -> Result<DataTree, ParseError> {
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
                Ok(DataTree::Text(text))
            }

            fn parse_inline_value(&mut self, s: &str, depth: usize) -> Result<DataTree, ParseError> {
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
                            message: yaml_flow_message(error).to_string(),
                        });
                }
                if s.starts_with('{') {
                    return parse_flow(s, depth, &mut self.budget)
                        .map(|(value, _)| value)
                        .map_err(|error| ParseError {
                            line: self.pos.saturating_add(1),
                            message: yaml_flow_message(error).to_string(),
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
        ) -> Result<(DataTree, usize), FlowError> {
            let chars: Vec<char> = s.chars().collect();
            parse_flow_at(&chars, 0, depth, budget)
        }
        fn parse_flow_at(
            chars: &[char],
            mut i: usize,
            depth: usize,
            budget: &mut YamlBudget,
        ) -> Result<(DataTree, usize), FlowError> {
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
                    Ok((DataTree::Array(items), i))
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
                        let key = key.trim().to_string();
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
                    Ok((DataTree::Object(entries), i))
                }
                _ => {
                    let (raw, ni) = scan_flow_scalar(chars, i, false);
                    Ok((yaml_scalar(raw.trim(), budget).map_err(|_| FlowError::Budget)?, ni))
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
        fn scalar_value(s: &str) -> DataTree {
            let s = s.trim();
            if s.is_empty() {
                return DataTree::Null;
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
                return DataTree::Text(out);
            }
            if s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2 {
                return DataTree::Text(s[1..s.len() - 1].replace("''", "'"));
            }
            match s {
                "null" | "Null" | "NULL" | "~" => return DataTree::Null,
                "true" | "True" | "TRUE" => return DataTree::Bool(true),
                "false" | "False" | "FALSE" => return DataTree::Bool(false),
                ".inf" | ".Inf" | ".INF" => return DataTree::Float(f64::INFINITY),
                "-.inf" | "-.Inf" => return DataTree::Float(f64::NEG_INFINITY),
                ".nan" | ".NaN" | ".NAN" => return DataTree::Float(f64::NAN),
                _ => {}
            }
            // Integer (decimal, with optional sign).
            if let Ok(n) = s.parse::<i64>() {
                return DataTree::Int(n);
            }
            // Float: must contain a `.`, `e`/`E` and parse cleanly.
            if (s.contains('.') || s.contains('e') || s.contains('E'))
                && s.chars()
                    .all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'))
            {
                if let Ok(f) = s.parse::<f64>() {
                    return DataTree::Float(f);
                }
            }
            DataTree::Text(s.to_string())
        }

        // ── Render: a `DataTree` → block YAML text ───────────────────────────────
        pub fn render(t: &DataTree) -> String {
            let mut out = String::new();
            render_node(t, 0, &mut out);
            let s = out.trim_end().to_string();
            if s.is_empty() {
                "{}".to_string()
            } else {
                s
            }
        }
        fn render_node(t: &DataTree, indent: usize, out: &mut String) {
            let pad = " ".repeat(indent);
            match t {
                DataTree::Object(entries) => {
                    if entries.is_empty() {
                        out.push_str(&format!("{}{{}}\n", pad));
                        return;
                    }
                    for (k, v) in entries {
                        match v {
                            DataTree::Object(e) if !e.is_empty() => {
                                out.push_str(&format!("{}{}:\n", pad, render_key(k)));
                                render_node(v, indent + 2, out);
                            }
                            DataTree::Array(a) if !a.is_empty() => {
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
                DataTree::Array(items) => render_seq(items, indent, out),
                _ => out.push_str(&format!("{}{}\n", pad, render_scalar(t))),
            }
        }
        fn render_seq(items: &[DataTree], indent: usize, out: &mut String) {
            let pad = " ".repeat(indent);
            for item in items {
                match item {
                    DataTree::Object(e) if !e.is_empty() => {
                        out.push_str(&format!("{}-\n", pad));
                        render_node(item, indent + 2, out);
                    }
                    DataTree::Array(a) if !a.is_empty() => {
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
        fn render_scalar(v: &DataTree) -> String {
            match v {
                DataTree::Null => "null".to_string(),
                DataTree::Bool(b) => b.to_string(),
                DataTree::Int(n) => n.to_string(),
                DataTree::Float(f) => format!("{:?}", f),
                DataTree::Number(_) | DataTree::TypedText(_) => {
                    unreachable!("internal JSON carrier escaped typed decode")
                }
                DataTree::Text(s) => {
                    if needs_quote(s) {
                        format!("{:?}", s)
                    } else {
                        s.clone()
                    }
                }
                DataTree::Bytes(bs) => {
                    let parts: Vec<String> = bs.iter().map(|b| b.to_string()).collect();
                    format!("[{}]", parts.join(", "))
                }
                // An inline collection value renders in flow form.
                DataTree::Array(items) => {
                    let parts: Vec<String> = items.iter().map(render_scalar).collect();
                    format!("[{}]", parts.join(", "))
                }
                DataTree::Object(entries) => {
                    let parts: Vec<String> = entries
                        .iter()
                        .map(|(k, val)| format!("{}: {}", render_key(k), render_scalar(val)))
                        .collect();
                    format!("{{{}}}", parts.join(", "))
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
