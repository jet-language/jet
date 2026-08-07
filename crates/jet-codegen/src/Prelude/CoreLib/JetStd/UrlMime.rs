    impl JetURL {
        pub fn parse(input: &String) -> Result<Self, String> {
            let raw = input.trim();
            let Some(colon) = raw.find(':') else {
                return Err("URL needs a scheme".to_string());
            };
            let scheme_raw = &raw[..colon];
            if !jet_url_valid_scheme(scheme_raw) {
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
                let (user, pass, h, p) = jet_url_parse_authority(authority)?;
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
            let mut url = JetURL {
                scheme,
                username,
                password,
                host,
                port,
                path,
                query,
                fragment,
            };
            url = url.normalize();
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
            let Some(host) = self.host.as_ref().filter(|h| !h.is_empty()) else {
                return String::new();
            };
            let mut out = String::new();
            let info = self.userinfo();
            if !info.is_empty() {
                out.push_str(&info);
                out.push('@');
            }
            out.push_str(host);
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
            if let Some(h) = &out.host {
                if let Ok(ascii) = jet_url_host_to_ascii(h) {
                    out.host = Some(ascii);
                }
            }
            if out.scheme != "data" {
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
            out.path = if path.starts_with('/') {
                path
            } else {
                let base = self.path.rsplit_once('/').map(|(b, _)| b).unwrap_or("");
                if base.is_empty() {
                    format!("/{}", path)
                } else {
                    format!("{}/{}", base, path)
                }
            };
            Ok(out.normalize())
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
                out.push_str(host);
                if let Some(port) = self.port {
                    out.push(':');
                    out.push_str(&port.to_string());
                }
            }
            if self.scheme == "data" && self.host.is_none() {
                out.push_str(&self.path);
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

    impl crate::JetShow for JetURL {
        fn jet_show(&self) -> String {
            self.to_string_value()
        }
    }

    impl JetMIME {
        pub fn parse(input: &String) -> Result<Self, String> {
            let mut parts = input.split(';');
            let essence = parts.next().unwrap_or("").trim();
            let Some((top, sub)) = essence.split_once('/') else {
                return Err("MIME type needs `type/subtype`".to_string());
            };
            let top = top.trim().to_ascii_lowercase();
            let sub = sub.trim().to_ascii_lowercase();
            if top.is_empty() || sub.is_empty() || !jet_mime_token(&top) || !jet_mime_token(&sub) {
                return Err(format!("invalid MIME type `{}`", essence));
            }
            let mut params = Vec::new();
            for p in parts {
                let p = p.trim();
                if p.is_empty() {
                    continue;
                }
                let Some((k, v)) = p.split_once('=') else {
                    return Err(format!("invalid MIME parameter `{}`", p));
                };
                let key = k.trim().to_ascii_lowercase();
                let val = v.trim().trim_matches('"').to_string();
                if key.is_empty() || !jet_mime_token(&key) {
                    return Err(format!("invalid MIME parameter `{}`", k.trim()));
                }
                params.push((key, val));
            }
            Ok(JetMIME { top, sub, params })
        }
        pub fn media_type(&self) -> String {
            self.top.clone()
        }
        pub fn subtype(&self) -> String {
            self.sub.clone()
        }
        pub fn essence(&self) -> String {
            format!("{}/{}", self.top, self.sub)
        }
        pub fn param(&self, name: &String) -> JetOutcome<String, JetAbsent> {
            let needle = name.to_ascii_lowercase();
            jet_outcome_of(self.params
                .iter()
                .find(|(k, _)| k == &needle)
                .map(|(_, v)| v.clone()))
        }
        pub fn params(&self) -> Vec<Vec<String>> {
            self.params
                .iter()
                .map(|(k, v)| vec![k.clone(), v.clone()])
                .collect()
        }
        pub fn to_string_value(&self) -> String {
            let mut out = self.essence();
            for (k, v) in &self.params {
                out.push_str("; ");
                out.push_str(k);
                out.push('=');
                out.push_str(v);
            }
            out
        }
    }

    impl crate::JetShow for JetMIME {
        fn jet_show(&self) -> String {
            self.to_string_value()
        }
    }

    fn jet_url_valid_scheme(s: &str) -> bool {
        let mut chars = s.chars();
        matches!(chars.next(), Some(c) if c.is_ascii_alphabetic())
            && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    }

    fn jet_url_parse_authority(
        authority: &str,
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
            let host = host_port[..=end].to_ascii_lowercase();
            let port = if host_port[end + 1..].starts_with(':') {
                Some(jet_url_parse_port(&host_port[end + 2..])?)
            } else {
                None
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

    fn jet_mime_token(s: &str) -> bool {
        !s.is_empty()
            && s.bytes().all(|b| {
                b.is_ascii_alphanumeric()
                    || matches!(
                        b,
                        b'!' | b'#'
                            | b'$'
                            | b'&'
                            | b'-'
                            | b'^'
                            | b'_'
                            | b'.'
                            | b'+'
                    )
            })
    }

    pub fn jet_mime_from_extension(ext: &str) -> Option<&'static str> {
        match ext.trim_start_matches('.').to_ascii_lowercase().as_str() {
            "html" | "htm" => Some("text/html"),
            "css" => Some("text/css"),
            "csv" => Some("text/csv"),
            "txt" | "text" => Some("text/plain"),
            "md" => Some("text/markdown"),
            "json" => Some("application/json"),
            "js" | "mjs" => Some("text/javascript"),
            "wasm" => Some("application/wasm"),
            "pdf" => Some("application/pdf"),
            "png" => Some("image/png"),
            "jpg" | "jpeg" => Some("image/jpeg"),
            "gif" => Some("image/gif"),
            "svg" => Some("image/svg+xml"),
            "webp" => Some("image/webp"),
            "ico" => Some("image/x-icon"),
            "mp3" => Some("audio/mpeg"),
            "mp4" => Some("video/mp4"),
            "xml" => Some("application/xml"),
            "zip" => Some("application/zip"),
            "gz" => Some("application/gzip"),
            "tar" => Some("application/x-tar"),
            _ => None,
        }
    }

    pub fn jet_extension_from_mime(mime: &str) -> Option<&'static str> {
        match mime.to_ascii_lowercase().split(';').next().unwrap_or("").trim() {
            "text/html" => Some("html"),
            "text/css" => Some("css"),
            "text/csv" => Some("csv"),
            "text/plain" => Some("txt"),
            "text/markdown" => Some("md"),
            "application/json" => Some("json"),
            "text/javascript" | "application/javascript" => Some("js"),
            "application/wasm" => Some("wasm"),
            "application/pdf" => Some("pdf"),
            "image/png" => Some("png"),
            "image/jpeg" => Some("jpg"),
            "image/gif" => Some("gif"),
            "image/svg+xml" => Some("svg"),
            "image/webp" => Some("webp"),
            "image/x-icon" => Some("ico"),
            "audio/mpeg" => Some("mp3"),
            "video/mp4" => Some("mp4"),
            "application/xml" | "text/xml" => Some("xml"),
            "application/zip" => Some("zip"),
            "application/gzip" => Some("gz"),
            "application/x-tar" => Some("tar"),
            _ => None,
        }
    }

