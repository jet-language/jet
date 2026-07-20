// D-HTTP-CORE2=A: one ordered, repeat-preserving header value shared by the
// client and server runtime paths.

#[derive(Clone, Default, PartialEq, Eq)]
struct JetHttpHeaders {
    entries: Vec<(String, String)>,
}

impl JetHttpHeaders {
    fn new() -> Self {
        Self::default()
    }

    fn valid_name(name: &str) -> bool {
        !name.is_empty()
            && name.bytes().all(|byte| {
                byte.is_ascii_alphanumeric()
                    || matches!(
                        byte,
                        b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+'
                            | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~'
                    )
            })
    }

    fn valid_value(value: &str) -> bool {
        !value.bytes().any(|byte| matches!(byte, b'\r' | b'\n' | b'\0'))
    }

    fn append(&mut self, name: &str, value: &str) -> Result<(), String> {
        if !Self::valid_name(name) {
            return Err(format!("invalid HTTP header name `{name}`"));
        }
        if !Self::valid_value(value) {
            return Err(format!("invalid value for HTTP header `{name}`"));
        }
        self.entries.push((name.to_string(), value.to_string()));
        Ok(())
    }

    fn set(&mut self, name: &str, value: &str) -> Result<(), String> {
        if !Self::valid_name(name) {
            return Err(format!("invalid HTTP header name `{name}`"));
        }
        if !Self::valid_value(value) {
            return Err(format!("invalid value for HTTP header `{name}`"));
        }
        self.remove(name);
        self.entries.push((name.to_string(), value.to_string()));
        Ok(())
    }

    fn first(&self, name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    fn get(&self, name: &str) -> Option<&String> {
        self.entries
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
            .map(|(_, value)| value)
    }

    fn all(&self, name: &str) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
            .collect()
    }

    fn remove(&mut self, name: &str) {
        self.entries
            .retain(|(candidate, _)| !candidate.eq_ignore_ascii_case(name));
    }

    fn to_flat(&self) -> Vec<String> {
        self.entries
            .iter()
            .flat_map(|(name, value)| [name.clone(), value.clone()])
            .collect()
    }

    fn from_flat(flat: Vec<String>) -> Result<Self, String> {
        if flat.len() % 2 != 0 {
            return Err("invalid flattened HTTP headers".to_string());
        }
        let mut headers = Self::new();
        for pair in flat.chunks_exact(2) {
            headers.append(&pair[0], &pair[1])?;
        }
        Ok(headers)
    }
}

impl std::fmt::Debug for JetHttpHeaders {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut list = formatter.debug_list();
        for (name, value) in &self.entries {
            let secret = matches!(
                name.to_ascii_lowercase().as_str(),
                "authorization" | "proxy-authorization" | "cookie" | "set-cookie"
            );
            list.entry(&(name, if secret { "<redacted>" } else { value.as_str() }));
        }
        list.finish()
    }
}

impl FromIterator<(String, String)> for JetHttpHeaders {
    fn from_iter<T: IntoIterator<Item = (String, String)>>(iter: T) -> Self {
        let mut headers = Self::new();
        for (name, value) in iter {
            headers
                .append(&name, &value)
                .expect("compiler-generated HTTP header is valid");
        }
        headers
    }
}

impl<'a> IntoIterator for &'a JetHttpHeaders {
    type Item = &'a (String, String);
    type IntoIter = std::slice::Iter<'a, (String, String)>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter()
    }
}
