    include!("Mime.rs");

    impl JetURL {
        pub fn parse(input: &String) -> Result<Self, String> {
            Self::parse_without_normalization(input).map(|url| url.normalize())
        }

        fn parse_without_normalization(input: &String) -> Result<Self, String> {
            Self::parse_without_normalization_with_marker(input, None)
        }

        fn parse_without_normalization_with_marker(
            input: &String,
            typed_marker: Option<&str>,
        ) -> Result<Self, String> {
            let raw = input.trim();
            let Some(colon) = raw.find(':') else {
                return Err("URL needs a scheme".to_string());
            };
            let scheme_raw = &raw[..colon];
            let marker_scheme = typed_marker.is_some_and(|marker| scheme_raw.contains(marker));
            if !marker_scheme && !jet_url_valid_scheme(scheme_raw) {
                return Err(format!("invalid URL scheme `{}`", scheme_raw));
            }
            let scheme = scheme_raw.to_ascii_lowercase();
            let mut rest = &raw[colon + 1..];
            let mut fragment = None;
            if let Some(i) = rest.find('#') {
                fragment = Some(jet_url_percent_decode_str(&rest[i + 1..])?);
                rest = &rest[..i];
            }
            let mut query = Vec::new();
            if let Some(i) = rest.find('?') {
                query = jet_url_parse_query(&rest[i + 1..])?;
                rest = &rest[..i];
            }
            let mut host = None;
            let mut port = None;
            let mut username = None;
            let mut password = None;
            let path;
            if let Some(after_slashes) = rest.strip_prefix("//") {
                let auth_end = after_slashes.find('/').unwrap_or(after_slashes.len());
                let authority = &after_slashes[..auth_end];
                let path_raw = &after_slashes[auth_end..];
                let (user, pass, h, p) = jet_url_parse_authority(authority, typed_marker)?;
                username = user;
                password = pass;
                host = Some(h);
                port = p;
                path = if path_raw.is_empty() {
                    "/".to_string()
                } else {
                    jet_url_percent_decode_str(path_raw)?
                };
            } else if matches!(scheme.as_str(), "http" | "https") {
                return Err(format!("{} URL needs `//host`", scheme));
            } else if scheme == "data" {
                path = rest.to_string();
            } else {
                path = jet_url_percent_decode_str(rest)?;
            }
            let url = JetURL {
                scheme,
                username,
                password,
                host,
                port,
                path,
                query,
                fragment,
                typed_host: None,
                typed_path: None,
            };
            Ok(url)
        }

        pub fn from_parts(
            scheme: &String,
            host: &String,
            path: &String,
            query: &Vec<Vec<String>>,
            fragment: &String,
        ) -> Result<Self, String> {
            if !jet_url_valid_scheme(scheme) {
                return Err(format!("invalid URL scheme `{}`", scheme));
            }
            let host = if host.is_empty() {
                None
            } else {
                Some(jet_url_host_to_ascii(host)?)
            };
            let fragment = if fragment.is_empty() {
                None
            } else {
                Some(fragment.clone())
            };
            Ok(JetURL {
                scheme: scheme.to_ascii_lowercase(),
                username: None,
                password: None,
                host,
                port: None,
                path: path.clone(),
                query: jet_url_pairs_from_rows(query),
                fragment,
                typed_host: None,
                typed_path: None,
            }
            .normalize())
        }

        pub fn file(path: &String) -> Self {
            JetURL {
                scheme: "file".to_string(),
                username: None,
                password: None,
                host: Some(String::new()),
                port: None,
                path: if path.starts_with('/') {
                    path.clone()
                } else {
                    format!("/{}", path)
                },
                query: Vec::new(),
                fragment: None,
                typed_host: None,
                typed_path: None,
            }
        }

        pub fn data(mime: &JetMIME, text: &String) -> Self {
            JetURL {
                scheme: "data".to_string(),
                username: None,
                password: None,
                host: None,
                port: None,
                path: format!(
                    "{},{}",
                    mime.to_string_value(),
                    jet_url_percent_encode(text, false)
                ),
                query: Vec::new(),
                fragment: None,
                typed_host: None,
                typed_path: None,
            }
        }

        pub fn scheme(&self) -> String {
            self.scheme.clone()
        }
        pub fn username(&self) -> String {
            self.username.clone().unwrap_or_default()
        }
        pub fn password(&self) -> String {
            self.password.clone().unwrap_or_default()
        }
        pub fn userinfo(&self) -> String {
            match (&self.username, &self.password) {
                (None, None) => String::new(),
                (Some(user), None) => user.clone(),
                (Some(user), Some(pass)) => format!("{}:{}", user, pass),
                (None, Some(pass)) => format!(":{}", pass),
            }
        }
        pub fn authority(&self) -> String {
            let raw_host = self.host.as_ref().filter(|h| !h.is_empty());
            if raw_host.is_none() && self.typed_host.is_none() {
                return String::new();
            }
            let mut out = String::new();
            let info = self.userinfo();
            if !info.is_empty() {
                out.push_str(&info);
                out.push('@');
            }
            if let Some(parts) = &self.typed_host {
                out.push_str(&jet_url_render_typed_parts(parts, false));
            } else if let Some(raw_host) = raw_host {
                out.push_str(raw_host);
            }
            if let Some(port) = self.port {
                out.push(':');
                out.push_str(&port.to_string());
            }
            out
        }
        pub fn host(&self) -> JetOutcome<String, JetAbsent> {
            jet_outcome_of(self.host.clone().filter(|h| !h.is_empty()))
        }
        pub fn port(&self) -> JetOutcome<i64, JetAbsent> {
            jet_outcome_of(self.port)
        }
        pub fn default_port(&self) -> JetOutcome<i64, JetAbsent> {
            match self.scheme.as_str() {
                "http" | "ws" => Ok(80),
                "https" | "wss" => Ok(443),
                "ftp" => Ok(21),
                "ssh" => Ok(22),
                "smtp" => Ok(25),
                "pop3" => Ok(110),
                "imap" => Ok(143),
                _ => Err(JetAbsent),
            }
        }
        pub fn path(&self) -> String {
            self.path.clone()
        }
        pub fn path_segments(&self) -> Vec<String> {
            if let Some(parts) = &self.typed_path {
                return jet_url_typed_path_segments(parts);
            }
            self.path
                .split('/')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect()
        }
        pub fn query(&self) -> String {
            jet_url_render_query(&self.query)
        }
        pub fn query_pairs(&self) -> Vec<Vec<String>> {
            self.query
                .iter()
                .map(|(k, v)| vec![k.clone(), v.clone()])
                .collect()
        }
        pub fn fragment(&self) -> JetOutcome<String, JetAbsent> {
            jet_outcome_of(self.fragment.clone())
        }
        pub fn normalize(&self) -> Self {
            let mut out = self.clone();
            out.scheme = out.scheme.to_ascii_lowercase();
            if out.typed_host.is_none() {
                if let Some(h) = &out.host {
                    if let Ok(ascii) = jet_url_host_to_ascii(h) {
                        out.host = Some(ascii);
                    }
                }
            }
            if out.typed_path.is_none() && out.scheme != "data" {
                out.path = jet_url_remove_dot_segments(&out.path);
            }
            out
        }
        pub fn join(&self, rel: &String) -> Result<Self, String> {
            if let Some(colon) = rel.find(':') {
                let before_slash = rel.find('/').map_or(true, |slash| colon < slash);
                if before_slash && jet_url_valid_scheme(&rel[..colon]) {
                    return JetURL::parse(rel);
                }
            }
            if rel.starts_with("//") {
                return JetURL::parse(&format!("{}:{}", self.scheme, rel));
            }
            let mut out = self.clone();
            let mut rest = rel.as_str();
            out.fragment = None;
            if let Some(i) = rest.find('#') {
                out.fragment = Some(jet_url_percent_decode_str(&rest[i + 1..])?);
                rest = &rest[..i];
            }
            out.query.clear();
            if let Some(i) = rest.find('?') {
                out.query = jet_url_parse_query(&rest[i + 1..])?;
                rest = &rest[..i];
            }
            let path = jet_url_percent_decode_str(rest)?;
            let path_is_empty = path.is_empty();
            let base_parts = self
                .typed_path
                .clone()
                .unwrap_or_else(|| vec![(self.path.clone(), false)]);
            let mut joined_parts = if path.starts_with('/') {
                vec![(path, false)]
            } else {
                let mut directory = jet_url_typed_path_directory(&base_parts);
                if directory.is_empty() {
                    directory.push(("/".to_string(), false));
                }
                if !path.is_empty() {
                    directory.push((path, false));
                }
                directory
            };
            if path_is_empty {
                out.typed_path = self.typed_path.clone();
                out.path = self.path.clone();
            } else {
                let typed_path = jet_url_normalize_typed_path(std::mem::take(&mut joined_parts));
                out.path = typed_path
                    .iter()
                    .map(|(part, _)| part.as_str())
                    .collect();
                out.typed_path = Some(typed_path);
            }
            // `typed_path` owns component boundaries. Normalizing a rendered
            // URL string here would erase hole boundaries before the next
            // operation can apply its component policy.
            Ok(out)
        }
        pub fn set_query(&self, key: &String, value: &String) -> Self {
            let mut out = self.clone();
            out.query.retain(|(k, _)| k != key);
            out.query.push((key.clone(), value.clone()));
            out
        }
        pub fn add_query(&self, key: &String, value: &String) -> Self {
            let mut out = self.clone();
            out.query.push((key.clone(), value.clone()));
            out
        }
        pub fn to_string_value(&self) -> String {
            let mut out = format!("{}:", self.scheme);
            if let Some(host) = &self.host {
                out.push_str("//");
                if let Some(user) = &self.username {
                    out.push_str(&jet_url_percent_encode(user, false));
                    if let Some(pass) = &self.password {
                        out.push(':');
                        out.push_str(&jet_url_percent_encode(pass, false));
                    }
                    out.push('@');
                } else if let Some(pass) = &self.password {
                    out.push(':');
                    out.push_str(&jet_url_percent_encode(pass, false));
                    out.push('@');
                }
                if let Some(parts) = &self.typed_host {
                    out.push_str(&jet_url_render_typed_parts(parts, false));
                } else {
                    out.push_str(host);
                }
                if let Some(port) = self.port {
                    out.push(':');
                    out.push_str(&port.to_string());
                }
            }
            if self.scheme == "data" && self.host.is_none() {
                if let Some(parts) = &self.typed_path {
                    out.push_str(&jet_url_render_typed_data_parts(parts));
                } else {
                    out.push_str(&self.path);
                }
            } else if let Some(parts) = &self.typed_path {
                out.push_str(&jet_url_render_typed_parts(parts, true));
            } else {
                out.push_str(&jet_url_percent_encode(&self.path, true));
            }
            if !self.query.is_empty() {
                out.push('?');
                out.push_str(&jet_url_render_query(&self.query));
            }
            if let Some(fragment) = &self.fragment {
                out.push('#');
                out.push_str(&jet_url_percent_encode(fragment, false));
            }
            out
        }
    }

    /// D-BOUND-HEAD1=A: validate the URL skeleton before lowering. Scheme and
    /// port are structural components, so interpolation cannot enter either.
    pub fn jet_validate_typed_url_literal(literals: &[&str]) -> Result<(), String> {
        if literals.is_empty() {
            return Err("URL literal needs a quoted body".to_string());
        }
        let marker = jet_typed_url_marker_name(literals);
        let skeleton = jet_typed_url_skeleton(literals, &marker);
        if let Some(colon) = skeleton.find(':') {
            let scheme_raw = &skeleton[..colon];
            if scheme_raw.contains(&marker) {
                return Err("URL scheme cannot contain interpolation".to_string());
            }
            if scheme_raw.eq_ignore_ascii_case("data") {
                let data_body = skeleton[colon + 1..]
                    .split(|ch| matches!(ch, '?' | '#'))
                    .next()
                    .unwrap_or_default();
                match data_body.find(',') {
                    Some(comma) if data_body[..comma].contains(&marker) => {
                        return Err("data URL MIME metadata cannot contain interpolation".to_string());
                    }
                    None if data_body.contains(&marker) => {
                        return Err("data URL payload needs a MIME separator".to_string());
                    }
                    _ => {}
                }
            }
            if let Some(authority) = jet_url_typed_authority(&skeleton[colon + 1..]) {
                let host_port = authority
                    .rsplit_once('@')
                    .map_or(authority, |(_, host_port)| host_port);
                let port_has_marker = if let Some(end) = host_port.find(']') {
                    host_port
                        .get(end + 1..)
                        .is_some_and(|suffix| suffix.starts_with(':') && suffix[1..].contains(&marker))
                } else {
                    host_port
                        .rsplit_once(':')
                        .is_some_and(|(_, port)| port.contains(&marker))
                };
                if port_has_marker {
                    return Err("URL port cannot contain interpolation".to_string());
                }
            }
        }
        JetURL::parse_without_normalization_with_marker(&skeleton, Some(&marker)).map(|_| ())
    }

    /// D-BOUND-HEAD1=A: URL heads parse the literal skeleton once. Hole values
    /// are assembled after parsing, so path and authority boundaries remain
    /// opaque to URL normalization and reparsing.
    pub fn jet_typed_url_literal(
        literals: &[&str],
        holes: Vec<String>,
    ) -> JetURL {
        let Ok(url) = jet_typed_url_literal_checked(literals, holes) else {
            // The parser/sema boundary rejects malformed heads before this
            // constructor is lowered. There is no user-facing runtime error
            // path to recover here, and manufacturing a URL would be worse
            // than terminating the violated compiler invariant.
            std::process::abort();
        };
        url
    }

    fn jet_typed_url_literal_checked(
        literals: &[&str],
        holes: Vec<String>,
    ) -> Result<JetURL, String> {
        if literals.len() != holes.len() + 1 {
            return Err("typed URL literal and hole counts do not match".to_string());
        }
        let (marker, mut url) = jet_typed_url_marker(literals)?;
        let mut hole_index = 0;
        let (scheme, _) = jet_url_apply_typed_holes(
            &url.scheme,
            &marker,
            &holes,
            &mut hole_index,
        )?;
        url.scheme = scheme.to_ascii_lowercase();
        if let Some(username) = url.username.take() {
            url.username = Some(jet_url_apply_typed_holes(
                &username,
                &marker,
                &holes,
                &mut hole_index,
            )?.0);
        }
        if let Some(password) = url.password.take() {
            url.password = Some(jet_url_apply_typed_holes(
                &password,
                &marker,
                &holes,
                &mut hole_index,
            )?.0);
        }
        if let Some(host) = url.host.take() {
            let (host, parts) = jet_url_apply_typed_holes(
                &host,
                &marker,
                &holes,
                &mut hole_index,
            )?;
            url.host = Some(host);
            url.typed_host = parts;
        }
        let (path, path_parts) = jet_url_apply_typed_holes(
            &url.path,
            &marker,
            &holes,
            &mut hole_index,
        )?;
        url.path = path;
        url.typed_path = path_parts;
        if let Some(parts) = url.typed_path.take() {
            let parts = if url.scheme == "data" {
                parts
            } else {
                jet_url_normalize_typed_path(parts)
            };
            url.path = parts
                .iter()
                .map(|(part, _)| part.as_str())
                .collect();
            url.typed_path = Some(parts);
        } else if url.scheme != "data" {
            url.path = jet_url_remove_dot_segments(&url.path);
        }
        for (key, value) in &mut url.query {
            let key_value = jet_url_apply_typed_holes(
                key,
                &marker,
                &holes,
                &mut hole_index,
            )?.0;
            *key = key_value;
            let value_value = jet_url_apply_typed_holes(
                value,
                &marker,
                &holes,
                &mut hole_index,
            )?.0;
            *value = value_value;
        }
        if let Some(fragment) = url.fragment.take() {
            url.fragment = Some(jet_url_apply_typed_holes(
                &fragment,
                &marker,
                &holes,
                &mut hole_index,
            )?.0);
        }
        if hole_index != holes.len() {
            return Err("typed URL holes do not match its literal skeleton".to_string());
        }
        Ok(url)
    }

    impl PartialEq for JetURL {
        fn eq(&self, other: &Self) -> bool {
            self.scheme == other.scheme
                && self.username == other.username
                && self.password == other.password
                && self.host == other.host
                && self.port == other.port
                && self.path == other.path
                && self.query == other.query
                && self.fragment == other.fragment
        }
    }

    impl crate::JetShow for JetURL {
        fn jet_show(&self) -> String {
            self.to_string_value()
        }
    }

    impl crate::JetDisplay for JetURL {
        fn jet_display(&self) -> String {
            self.to_string_value()
        }
    }

    impl crate::JetDebug for JetURL {
        fn jet_debug(&self) -> String {
            self.to_string_value()
        }
    }

    impl JetMIME {
        pub fn parse(input: &String) -> Result<Self, String> {
            let parts = jet_mime_parse_parts(input)?;
            Ok(JetMIME { top: parts.top, sub: parts.sub, params: parts.params })
        }
        pub fn media_type(&self) -> String {
            self.top.clone()
        }
        pub fn subtype(&self) -> String {
            self.sub.clone()
        }
        pub fn essence(&self) -> String {
            jet_mime_essence(&self.top, &self.sub)
        }
        pub fn param(&self, name: &String) -> JetOutcome<String, JetAbsent> {
            jet_outcome_of(jet_mime_param(&self.params, name).map(str::to_string))
        }
        pub fn params(&self) -> Vec<Vec<String>> {
            self.params
                .iter()
                .map(|(k, v)| vec![k.clone(), v.clone()])
                .collect()
        }
        pub fn to_string_value(&self) -> String {
            jet_mime_to_string(&self.top, &self.sub, &self.params)
        }
    }

    impl crate::JetShow for JetMIME {
        fn jet_show(&self) -> String {
            self.to_string_value()
        }
    }

    impl crate::JetDisplay for JetMIME {
        fn jet_display(&self) -> String {
            self.to_string_value()
        }
    }

    impl crate::JetDebug for JetMIME {
        fn jet_debug(&self) -> String {
            self.to_string_value()
        }
    }

    fn jet_typed_url_marker_name(literals: &[&str]) -> String {
        let mut nonce = 0;
        loop {
            let marker = format!("jet-hole-opaque-{nonce}-");
            if !literals.iter().any(|literal| literal.contains(&marker)) {
                return marker;
            }
            nonce += 1;
        }
    }

    fn jet_typed_url_marker(literals: &[&str]) -> Result<(String, JetURL), String> {
        let marker = jet_typed_url_marker_name(literals);
        let skeleton = jet_typed_url_skeleton(literals, &marker);
        let url = JetURL::parse_without_normalization_with_marker(&skeleton, Some(&marker))?;
        Ok((marker, url))
    }

    fn jet_url_typed_authority(rest: &str) -> Option<&str> {
        let rest = rest.strip_prefix("//")?;
        let end = rest
            .char_indices()
            .find_map(|(index, ch)| matches!(ch, '/' | '?' | '#').then_some(index))
            .unwrap_or(rest.len());
        Some(&rest[..end])
    }

    fn jet_typed_url_skeleton(literals: &[&str], marker: &str) -> String {
        let mut skeleton = String::new();
        for (index, literal) in literals.iter().enumerate() {
            skeleton.push_str(literal);
            if index + 1 < literals.len() {
                skeleton.push_str(marker);
            }
        }
        skeleton
    }

    fn jet_url_apply_typed_holes(
        value: &str,
        marker: &str,
        holes: &[String],
        hole_index: &mut usize,
    ) -> Result<(String, Option<Vec<(String, bool)>>), String> {
        let mut parts = Vec::new();
        let mut cursor = 0;
        while let Some(offset) = value[cursor..].find(marker) {
            let start = cursor + offset;
            let literal = &value[cursor..start];
            if !literal.is_empty() || parts.is_empty() {
                parts.push((literal.to_string(), false));
            }
            let hole = holes
                .get(*hole_index)
                .ok_or_else(|| "typed URL holes do not match its literal skeleton".to_string())?
                .clone();
            parts.push((hole, true));
            *hole_index += 1;
            cursor = start + marker.len();
        }
        if parts.is_empty() {
            return Ok((value.to_string(), None));
        }
        let tail = &value[cursor..];
        if !tail.is_empty() {
            parts.push((tail.to_string(), false));
        }
        let assembled = parts
            .iter()
            .map(|(part, _)| part.as_str())
            .collect::<String>();
        Ok((assembled, Some(parts)))
    }

    fn jet_url_render_data_mime_literal(value: &str) -> String {
        let mut out = String::new();
        for byte in value.bytes() {
            if byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#' | b'$' | b'&' | b'+' | b'-' | b'/' | b';' | b'=' | b'^'
                        | b'_' | b'.'
                )
            {
                out.push(byte as char);
            } else {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
        out
    }

    /// Data URLs have two grammars in one path: MIME metadata before the
    /// first comma, payload after it. Keep literal MIME separators intact and
    /// percent-encode only interpolated payload data.
    fn jet_url_render_typed_data_parts(parts: &[(String, bool)]) -> String {
        let mut out = String::new();
        let mut payload = false;
        for (part, hole) in parts {
            if payload {
                if *hole {
                    out.push_str(&jet_url_percent_encode(part, false));
                } else {
                    out.push_str(part);
                }
                continue;
            }
            if *hole {
                out.push_str(&jet_url_percent_encode(part, false));
                continue;
            }
            if let Some(comma) = part.find(',') {
                out.push_str(&jet_url_render_data_mime_literal(&part[..comma]));
                out.push(',');
                payload = true;
                out.push_str(&part[comma + 1..]);
            } else {
                out.push_str(&jet_url_render_data_mime_literal(part));
            }
        }
        out
    }

    fn jet_url_render_typed_parts(parts: &[(String, bool)], literal_path: bool) -> String {
        parts
            .iter()
            .map(|(part, hole)| {
                if literal_path && *hole && matches!(part.as_str(), "." | "..") {
                    return part
                        .as_bytes()
                        .iter()
                        .map(|byte| format!("%{byte:02X}"))
                        .collect();
                }
                if !literal_path && !*hole {
                    jet_url_percent_encode_authority_literal(part)
                } else {
                    jet_url_percent_encode(part, literal_path && !*hole)
                }
            })
            .collect::<String>()
    }

    fn jet_url_percent_encode_authority_literal(s: &str) -> String {
        let mut out = String::new();
        for byte in s.bytes() {
            let keep = byte.is_ascii_alphanumeric()
                || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'[' | b']' | b':');
            if keep {
                out.push(byte as char);
            } else {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
        out
    }

    fn jet_url_normalize_typed_path(
        parts: Vec<(String, bool)>,
    ) -> Vec<(String, bool)> {
        let absolute = parts
            .first()
            .is_some_and(|(part, hole)| !*hole && part.starts_with('/'));
        let trailing = parts
            .last()
            .is_some_and(|(part, hole)| !*hole && part.ends_with('/'));
        let mut segments: Vec<Vec<(String, bool)>> = vec![Vec::new()];
        for (part, hole) in parts {
            if hole {
                if let Some(current) = segments.last_mut() {
                    current.push((part, true));
                }
                continue;
            }
            for (index, piece) in part.split('/').enumerate() {
                if index > 0 {
                    segments.push(Vec::new());
                }
                if !piece.is_empty() {
                    if let Some(current) = segments.last_mut() {
                        current.push((piece.to_string(), false));
                    }
                }
            }
        }
        let mut normalized: Vec<Vec<(String, bool)>> = Vec::new();
        for segment in segments {
            if segment.is_empty() {
                continue;
            }
            let has_hole = segment.iter().any(|(_, hole)| *hole);
            let value = segment
                .iter()
                .map(|(part, _)| part.as_str())
                .collect::<String>();
            if !has_hole && value == "." {
                continue;
            }
            if !has_hole && value == ".." {
                match normalized.last() {
                    Some(previous)
                        if !previous.iter().any(|(_, hole)| *hole) =>
                    {
                        normalized.pop();
                    }
                    Some(_) => normalized.push(segment),
                    None => {}
                }
                continue;
            }
            normalized.push(segment);
        }
        let mut out = Vec::new();
        if absolute {
            out.push(("/".to_string(), false));
        }
        for (index, segment) in normalized.into_iter().enumerate() {
            if index > 0 {
                out.push(("/".to_string(), false));
            }
            out.extend(segment);
        }
        if trailing && !out.last().is_some_and(|(part, _)| part.ends_with('/')) {
            out.push(("/".to_string(), false));
        }
        out
    }

    fn jet_url_typed_path_directory(parts: &[(String, bool)]) -> Vec<(String, bool)> {
        let Some((last_index, slash)) = parts.iter().enumerate().rev().find_map(
            |(index, (part, hole))| {
                (!*hole)
                    .then(|| part.rfind('/').map(|slash| (index, slash)))
                    .flatten()
            },
        ) else {
            return Vec::new();
        };
        let mut directory = parts[..last_index].to_vec();
        let literal = &parts[last_index].0;
        directory.push((literal[..=slash].to_string(), false));
        directory
    }

    fn jet_url_typed_path_segments(parts: &[(String, bool)]) -> Vec<String> {
        let mut segments = Vec::new();
        let mut current = String::new();
        for (part, hole) in parts {
            if *hole {
                current.push_str(part);
                continue;
            }
            for (index, piece) in part.split('/').enumerate() {
                if index > 0 && !current.is_empty() {
                    segments.push(std::mem::take(&mut current));
                }
                current.push_str(piece);
            }
        }
        if !current.is_empty() {
            segments.push(current);
        }
        segments
    }

    fn jet_url_valid_scheme(s: &str) -> bool {
        let mut chars = s.chars();
        matches!(chars.next(), Some(c) if c.is_ascii_alphabetic())
            && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    }

    fn jet_url_parse_authority(
        authority: &str,
        typed_marker: Option<&str>,
    ) -> Result<(Option<String>, Option<String>, String, Option<i64>), String> {
        let (userinfo, host_port) = match authority.rsplit_once('@') {
            Some((ui, hp)) => (Some(ui), hp),
            None => (None, authority),
        };
        let (username, password) = if let Some(ui) = userinfo {
            match ui.split_once(':') {
                Some((user, pass)) => (
                    Some(jet_url_percent_decode_str(user)?),
                    Some(jet_url_percent_decode_str(pass)?),
                ),
                None => (Some(jet_url_percent_decode_str(ui)?), None),
            }
        } else {
            (None, None)
        };
        if host_port.is_empty() {
            return Err("URL host is empty".to_string());
        }
        if host_port.starts_with('[') {
            let Some(end) = host_port.find(']') else {
                return Err("IPv6 host is missing `]`".to_string());
            };
            let suffix = &host_port[end + 1..];
            let (host, port) = if suffix.is_empty() {
                (host_port[..=end].to_ascii_lowercase(), None)
            } else if let Some(marker) = typed_marker {
                if suffix.starts_with(':')
                    && jet_url_only_typed_markers(&suffix[1..], marker)
                {
                    // Validation rejects port holes. Keep marker parsing
                    // total so a rejected head cannot panic before its
                    // registered diagnostic is emitted.
                    (host_port.to_ascii_lowercase(), None)
                } else if jet_url_only_typed_markers(suffix, marker) {
                    // Preserve trailing typed parts after `]` for the same
                    // authority-boundary rule.
                    (host_port.to_ascii_lowercase(), None)
                } else if suffix.starts_with(':') {
                    (
                        host_port[..=end].to_ascii_lowercase(),
                        Some(jet_url_parse_port(&suffix[1..])?),
                    )
                } else {
                    return Err("invalid text after IPv6 host".to_string());
                }
            } else if suffix.starts_with(':') {
                (
                    host_port[..=end].to_ascii_lowercase(),
                    Some(jet_url_parse_port(&suffix[1..])?),
                )
            } else {
                return Err("invalid text after IPv6 host".to_string());
            };
            return Ok((username, password, host, port));
        }
        let (host, port) = if let Some((h, p)) = host_port.rsplit_once(':') {
            if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) {
                (h, Some(jet_url_parse_port(p)?))
            } else {
                (host_port, None)
            }
        } else {
            (host_port, None)
        };
        Ok((username, password, jet_url_host_to_ascii(host)?, port))
    }

    fn jet_url_only_typed_markers(value: &str, marker: &str) -> bool {
        if marker.is_empty() {
            return false;
        }
        let mut rest = value;
        while let Some(next) = rest.strip_prefix(marker) {
            rest = next;
        }
        rest.is_empty()
    }

    fn jet_url_parse_port(p: &str) -> Result<i64, String> {
        let n: i64 = p.parse().map_err(|_| format!("invalid URL port `{}`", p))?;
        if !(0..=65535).contains(&n) {
            return Err(format!("URL port out of range `{}`", p));
        }
        Ok(n)
    }

    fn jet_url_host_to_ascii(host: &str) -> Result<String, String> {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        let mut labels = Vec::new();
        for label in host.split('.') {
            if label.is_empty() {
                return Err("URL host has an empty label".to_string());
            }
            labels.push(if label.is_ascii() {
                label.to_string()
            } else {
                format!("xn--{}", jet_punycode_encode(label)?)
            });
        }
        Ok(labels.join("."))
    }

    fn jet_punycode_encode(input: &str) -> Result<String, String> {
        const BASE: u32 = 36;
        const TMIN: u32 = 1;
        const TMAX: u32 = 26;
        const SKEW: u32 = 38;
        const DAMP: u32 = 700;
        const INITIAL_BIAS: u32 = 72;
        const INITIAL_N: u32 = 128;
        let codepoints: Vec<u32> = input.chars().map(|c| c as u32).collect();
        let mut out = String::new();
        for &cp in &codepoints {
            if cp < 0x80 {
                out.push(char::from_u32(cp).ok_or_else(|| "bad codepoint".to_string())?);
            }
        }
        let basic = out.chars().count() as u32;
        let mut handled = basic;
        if basic > 0 && handled < codepoints.len() as u32 {
            out.push('-');
        }
        let mut n = INITIAL_N;
        let mut delta = 0u32;
        let mut bias = INITIAL_BIAS;
        while handled < codepoints.len() as u32 {
            let m = *codepoints
                .iter()
                .filter(|&&cp| cp >= n)
                .min()
                .ok_or_else(|| "bad punycode input".to_string())?;
            delta = delta
                .checked_add((m - n).saturating_mul(handled + 1))
                .ok_or_else(|| "punycode overflow".to_string())?;
            n = m;
            for &cp in &codepoints {
                if cp < n {
                    delta = delta.checked_add(1).ok_or_else(|| "punycode overflow".to_string())?;
                } else if cp == n {
                    let mut q = delta;
                    let mut k = BASE;
                    loop {
                        let t = if k <= bias {
                            TMIN
                        } else if k >= bias + TMAX {
                            TMAX
                        } else {
                            k - bias
                        };
                        if q < t {
                            break;
                        }
                        out.push(jet_punycode_digit(t + ((q - t) % (BASE - t)))?);
                        q = (q - t) / (BASE - t);
                        k += BASE;
                    }
                    out.push(jet_punycode_digit(q)?);
                    bias = jet_punycode_adapt(delta, handled + 1, handled == basic);
                    delta = 0;
                    handled += 1;
                }
            }
            delta = delta.checked_add(1).ok_or_else(|| "punycode overflow".to_string())?;
            n = n.checked_add(1).ok_or_else(|| "punycode overflow".to_string())?;
        }
        Ok(out)
    }

    fn jet_punycode_digit(d: u32) -> Result<char, String> {
        char::from_u32(if d < 26 { b'a' as u32 + d } else { b'0' as u32 + d - 26 })
            .ok_or_else(|| "bad punycode digit".to_string())
    }

    fn jet_punycode_adapt(mut delta: u32, points: u32, first: bool) -> u32 {
        delta = if first { delta / 700 } else { delta / 2 };
        delta += delta / points;
        let mut k = 0;
        while delta > ((36 - 1) * 26) / 2 {
            delta /= 36 - 1;
            k += 36;
        }
        k + (((36 - 1 + 1) * delta) / (delta + 38))
    }

    fn jet_url_parse_query(q: &str) -> Result<Vec<(String, String)>, String> {
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for part in q.split('&') {
            let (k, v) = part.split_once('=').unwrap_or((part, ""));
            out.push((jet_url_percent_decode_str(k)?, jet_url_percent_decode_str(v)?));
        }
        Ok(out)
    }

    fn jet_url_pairs_from_rows(rows: &Vec<Vec<String>>) -> Vec<(String, String)> {
        rows.iter()
            .filter(|r| !r.is_empty())
            .map(|r| {
                (
                    r.get(0).cloned().unwrap_or_default(),
                    r.get(1).cloned().unwrap_or_default(),
                )
            })
            .collect()
    }

    fn jet_url_render_query(pairs: &[(String, String)]) -> String {
        pairs
            .iter()
            .map(|(k, v)| format!(
                "{}={}",
                jet_url_percent_encode(k, false),
                jet_url_percent_encode(v, false)
            ))
            .collect::<Vec<_>>()
            .join("&")
    }

    pub fn jet_url_percent_encode(s: &str, path: bool) -> String {
        let mut out = String::new();
        for b in s.as_bytes() {
            let keep = b.is_ascii_alphanumeric()
                || matches!(*b, b'-' | b'.' | b'_' | b'~')
                || (path && *b == b'/');
            if keep {
                out.push(*b as char);
            } else {
                out.push_str(&format!("%{:02X}", b));
            }
        }
        out
    }

    pub fn jet_url_percent_decode_str(s: &str) -> Result<String, String> {
        let bytes = s.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' {
                if i + 2 >= bytes.len() {
                    return Err("truncated percent escape".to_string());
                }
                let hex = &s[i + 1..i + 3];
                let byte = u8::from_str_radix(hex, 16)
                    .map_err(|_| format!("invalid percent escape `%{}`", hex))?;
                out.push(byte);
                i += 3;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        String::from_utf8(out).map_err(|_| "percent escape is not valid UTF-8".to_string())
    }

    fn jet_url_remove_dot_segments(path: &str) -> String {
        if path.is_empty() || !path.contains('.') {
            return path.to_string();
        }
        let absolute = path.starts_with('/');
        let trailing = path.ends_with('/');
        let mut parts = Vec::new();
        for p in path.split('/') {
            match p {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                _ => parts.push(p),
            }
        }
        let mut out = String::new();
        if absolute {
            out.push('/');
        }
        out.push_str(&parts.join("/"));
        if trailing && !out.ends_with('/') {
            out.push('/');
        }
        if out.is_empty() && absolute {
            "/".to_string()
        } else {
            out
        }
    }
