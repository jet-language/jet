// D-EMAIL1=A: bounded, dependency-free email address and MIME substrate.
pub mod jet_email {
    pub const MAX_RECIPIENTS: usize = 100;
    pub const MAX_ATTACHMENTS: usize = 64;
    pub const MAX_HEADER_BYTES: usize = 998;
    pub const MAX_HEADER_VALUE_BYTES: usize = 64 * 1024;
    pub const MAX_BODY_BYTES: usize = 1024 * 1024;
    pub const MAX_ATTACHMENT_BYTES: usize = 25 * 1024 * 1024;
    pub const MAX_MESSAGE_BYTES: usize = 32 * 1024 * 1024;

    #[derive(Clone, Debug, PartialEq)]
    pub struct Error {
        pub kind: &'static str,
        pub message: String,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct Address {
        display: Option<String>,
        mailbox: String,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct Attachment {
        filename: String,
        mime: String,
        bytes: Vec<u8>,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct Message {
        from: Address,
        to: Vec<Address>,
        bcc: Vec<Address>,
        subject: String,
        text: String,
        html: String,
        attachments: Vec<Attachment>,
        wire_upper: usize,
    }

    fn error(kind: &'static str, message: impl Into<String>) -> Error {
        Error { kind, message: message.into() }
    }

    fn reject_controls(value: &str, what: &str) -> Result<(), Error> {
        if value.chars().any(char::is_control) {
            return Err(error("InvalidHeader", format!("{what} contains a forbidden control character")));
        }
        Ok(())
    }

    pub fn address(input: &String) -> Result<Address, Error> {
        reject_controls(input, "email address")?;
        let value = input.trim();
        if value.is_empty() || value.len() > 512 {
            return Err(error("InvalidAddress", "email address must contain 1 to 512 bytes"));
        }
        let opens = value.bytes().filter(|byte| *byte == b'<').count();
        let closes = value.bytes().filter(|byte| *byte == b'>').count();
        let (display, mailbox) = match (opens, closes) {
            (0, 0) => (None, value),
            (1, 1) if value.ends_with('>') => {
                let open = value.rfind('<').unwrap();
                let shown = value[..open].trim();
                if shown.is_empty() {
                    return Err(error("InvalidAddress", "display name cannot be empty"));
                }
                (Some(parse_display(shown)?), value[open + 1..value.len() - 1].trim())
            }
            _ => return Err(error("InvalidAddress", "display address must have one final `<mailbox>`")),
        };
        validate_mailbox(mailbox)?;
        Ok(Address { display, mailbox: mailbox.to_string() })
    }

    fn parse_display(value: &str) -> Result<String, Error> {
        if !value.starts_with('"') {
            if value.contains('"') || value.contains('<') || value.contains('>') {
                return Err(error("InvalidAddress", "display name has an unmatched quote or angle bracket"));
            }
            return Ok(value.to_string());
        }
        if value.len() < 2 || !value.ends_with('"') {
            return Err(error("InvalidAddress", "quoted display name needs a closing quote"));
        }
        let mut out = String::new();
        let mut escaped = false;
        for ch in value[1..value.len() - 1].chars() {
            if escaped {
                if ch != '"' && ch != '\\' {
                    return Err(error("InvalidAddress", "quoted display name may escape only quote or backslash"));
                }
                out.push(ch);
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                return Err(error("InvalidAddress", "quoted display name contains an unescaped quote"));
            } else {
                out.push(ch);
            }
        }
        if escaped || out.is_empty() {
            return Err(error("InvalidAddress", "quoted display name is empty or ends with an escape"));
        }
        Ok(out)
    }

    fn validate_mailbox(mailbox: &str) -> Result<(), Error> {
        if mailbox.is_empty() || !mailbox.is_ascii() || mailbox.len() > 254 {
            return Err(error("InvalidAddress", "mailbox must be 1 to 254 ASCII bytes"));
        }
        let at = mailbox_separator(mailbox)?;
        let local = &mailbox[..at];
        let domain = &mailbox[at + 1..];
        if local.is_empty() || domain.is_empty() || local.len() > 64 || domain.len() > 253 {
            return Err(error("InvalidAddress", "mailbox local part or domain has an invalid length"));
        }
        if local.starts_with('"') {
            validate_quoted_local(local)?;
        } else if local.starts_with('.') || local.ends_with('.') || local.contains("..")
            || !local.bytes().all(is_atext)
        {
            return Err(error("InvalidAddress", "mailbox local part is not dot-atom or quoted-string"));
        }
        if domain.starts_with('.') || domain.ends_with('.') || domain.contains("..")
            || domain.split('.').any(|label| {
                label.is_empty() || label.starts_with('-') || label.ends_with('-')
                    || !label.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            })
        {
            return Err(error("InvalidAddress", "mailbox domain has an invalid label"));
        }
        Ok(())
    }

    fn mailbox_separator(mailbox: &str) -> Result<usize, Error> {
        let mut quoted = false;
        let mut escaped = false;
        let mut separator = None;
        for (index, byte) in mailbox.bytes().enumerate() {
            if escaped {
                escaped = false;
            } else if quoted && byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = !quoted;
            } else if byte == b'@' && !quoted {
                if separator.replace(index).is_some() {
                    return Err(error("InvalidAddress", "mailbox needs exactly one unquoted `@`"));
                }
            }
        }
        if quoted || escaped {
            return Err(error("InvalidAddress", "mailbox has an unterminated quoted local part"));
        }
        separator.ok_or_else(|| error("InvalidAddress", "mailbox needs one unquoted `@`"))
    }

    fn validate_quoted_local(local: &str) -> Result<(), Error> {
        if local.len() < 2 || !local.ends_with('"') {
            return Err(error("InvalidAddress", "quoted mailbox local part needs a closing quote"));
        }
        let mut escaped = false;
        for byte in local[1..local.len() - 1].bytes() {
            if escaped {
                if !(33..=126).contains(&byte) {
                    return Err(error("InvalidAddress", "quoted mailbox escape is not printable ASCII"));
                }
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' || !(32..=126).contains(&byte) {
                return Err(error("InvalidAddress", "quoted mailbox local part contains an invalid byte"));
            }
        }
        if escaped {
            return Err(error("InvalidAddress", "quoted mailbox local part ends with an escape"));
        }
        Ok(())
    }

    fn is_atext(byte: u8) -> bool {
        byte.is_ascii_alphanumeric() || matches!(byte, b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'/' | b'=' | b'?' | b'^' | b'_' | b'`' | b'{' | b'|' | b'}' | b'~' | b'.')
    }

    pub fn attachment(filename: &String, mime: &String, bytes: &Vec<u8>) -> Result<Attachment, Error> {
        reject_controls(filename, "attachment filename")?;
        reject_controls(mime, "attachment content type")?;
        if filename.trim().is_empty() || filename.contains('/') || filename.contains('\\') {
            return Err(error("InvalidAttachment", "attachment filename must be a plain non-empty name"));
        }
        if !valid_mime(mime) {
            return Err(error("InvalidAttachment", "attachment content type must be `type/subtype`"));
        }
        ensure_physical_header_len("Content-Type", mime.len())?;
        let disposition_len = "attachment; filename*=UTF-8''".len()
            .checked_add(percent_encoded_len(filename)?)
            .ok_or_else(|| error("LimitExceeded", "attachment header length overflow"))?;
        ensure_physical_header_len("Content-Disposition", disposition_len)?;
        if bytes.len() > MAX_ATTACHMENT_BYTES {
            return Err(error("LimitExceeded", format!("attachment exceeds {MAX_ATTACHMENT_BYTES} bytes")));
        }
        Ok(Attachment { filename: filename.clone(), mime: mime.to_ascii_lowercase(), bytes: bytes.clone() })
    }

    pub fn message(
        from: &Address,
        to: &Vec<Address>,
        bcc: &Vec<Address>,
        subject: &String,
        text: &String,
        html: &String,
        attachments: &Vec<Attachment>,
    ) -> Result<Message, Error> {
        reject_controls(subject, "subject")?;
        if subject.len() > MAX_HEADER_VALUE_BYTES {
            return Err(error("LimitExceeded", format!("subject exceeds {MAX_HEADER_VALUE_BYTES} bytes")));
        }
        if to.is_empty() {
            return Err(error("InvalidMessage", "message needs at least one visible recipient"));
        }
        let recipients = to.len().checked_add(bcc.len())
            .ok_or_else(|| error("LimitExceeded", "recipient count overflow"))?;
        if recipients > MAX_RECIPIENTS {
            return Err(error("LimitExceeded", format!("message exceeds {MAX_RECIPIENTS} recipients")));
        }
        if attachments.len() > MAX_ATTACHMENTS {
            return Err(error("LimitExceeded", format!("message exceeds {MAX_ATTACHMENTS} attachments")));
        }
        if text.is_empty() && html.is_empty() {
            return Err(error("InvalidMessage", "message needs text or HTML content"));
        }
        if text.len() > MAX_BODY_BYTES || html.len() > MAX_BODY_BYTES {
            return Err(error("LimitExceeded", format!("each message body is limited to {MAX_BODY_BYTES} bytes")));
        }
        let wire_upper = prospective_wire_upper(from, to, subject, text, html, attachments)?;
        if wire_upper > MAX_MESSAGE_BYTES {
            return Err(error("LimitExceeded", format!("serialized message exceeds {MAX_MESSAGE_BYTES} bytes")));
        }
        ensure_rendered_address_header("From", std::slice::from_ref(from))?;
        ensure_rendered_address_header("To", to)?;
        ensure_encoded_header_lines("Subject", subject)?;
        Ok(Message {
            from: from.clone(), to: to.clone(), bcc: bcc.clone(), subject: subject.clone(),
            text: text.clone(), html: html.clone(), attachments: attachments.clone(), wire_upper,
        })
    }

    fn prospective_wire_upper(
        from: &Address,
        to: &[Address],
        subject: &str,
        text: &str,
        html: &str,
        attachments: &[Attachment],
    ) -> Result<usize, Error> {
        let mut total = 4096usize;
        checked_add(&mut total, rendered_address_len(from))?;
        for address in to { checked_add(&mut total, rendered_address_len(address).saturating_add(4))?; }
        checked_add(&mut total, encoded_header_len(subject).saturating_add(32))?;
        checked_add(&mut total, base64_lines_len(text.len()).saturating_add(256))?;
        checked_add(&mut total, base64_lines_len(html.len()).saturating_add(256))?;
        for item in attachments {
            checked_add(&mut total, base64_lines_len(item.bytes.len()))?;
            checked_add(&mut total, item.mime.len())?;
            checked_add(&mut total, percent_encoded_len(&item.filename)?)?;
            checked_add(&mut total, 512)?;
        }
        checked_add(&mut total, attachments.len().saturating_mul(256))?;
        Ok(total)
    }

    fn checked_add(total: &mut usize, add: usize) -> Result<(), Error> {
        *total = total.checked_add(add).ok_or_else(|| error("LimitExceeded", "message size overflow"))?;
        Ok(())
    }

    pub fn serialize(message: &Message) -> Result<Vec<u8>, Error> {
        let mixed = boundary(message, "mixed");
        let alternative = boundary(message, "alternative");
        let mut out = String::with_capacity(message.wire_upper.min(MAX_MESSAGE_BYTES));
        push_header(&mut out, "From", &render_address(&message.from))?;
        push_header(&mut out, "To", &render_addresses(&message.to, "To"))?;
        push_header(&mut out, "Subject", &encode_header(&message.subject))?;
        push_header(&mut out, "MIME-Version", "1.0")?;
        if message.attachments.is_empty() {
            render_body(&mut out, message, &alternative)?;
        } else {
            push_header(&mut out, "Content-Type", &format!("multipart/mixed; boundary=\"{mixed}\""))?;
            out.push_str("\r\n");
            out.push_str(&format!("--{mixed}\r\n"));
            render_body(&mut out, message, &alternative)?;
            for item in &message.attachments {
                out.push_str(&format!("\r\n--{mixed}\r\n"));
                push_header(&mut out, "Content-Type", &item.mime)?;
                push_header(&mut out, "Content-Transfer-Encoding", "base64")?;
                push_header(&mut out, "Content-Disposition", &format!("attachment; filename*=UTF-8''{}", percent_encode(&item.filename)?))?;
                out.push_str("\r\n");
                out.push_str(&base64_lines(&item.bytes));
            }
            out.push_str(&format!("\r\n--{mixed}--\r\n"));
        }
        if out.len() > message.wire_upper || out.len() > MAX_MESSAGE_BYTES {
            return Err(error("LimitExceeded", "serialized message exceeded its checked wire bound"));
        }
        Ok(out.into_bytes())
    }

    fn render_body(out: &mut String, message: &Message, alternative: &str) -> Result<(), Error> {
        if message.html.is_empty() {
            text_part(out, "text/plain", &message.text)?;
        } else if message.text.is_empty() {
            text_part(out, "text/html", &message.html)?;
        } else {
            push_header(out, "Content-Type", &format!("multipart/alternative; boundary=\"{alternative}\""))?;
            out.push_str("\r\n");
            out.push_str(&format!("--{alternative}\r\n"));
            text_part(out, "text/plain", &message.text)?;
            out.push_str(&format!("\r\n--{alternative}\r\n"));
            text_part(out, "text/html", &message.html)?;
            out.push_str(&format!("\r\n--{alternative}--\r\n"));
        }
        Ok(())
    }

    fn text_part(out: &mut String, mime: &str, body: &str) -> Result<(), Error> {
        push_header(out, "Content-Type", &format!("{mime}; charset=utf-8"))?;
        push_header(out, "Content-Transfer-Encoding", "base64")?;
        out.push_str("\r\n");
        out.push_str(&base64_lines(body.as_bytes()));
        Ok(())
    }

    fn push_header(out: &mut String, name: &str, value: &str) -> Result<(), Error> {
        validate_folded_header(name, value)?;
        out.push_str(name);
        out.push_str(": ");
        out.push_str(value);
        out.push_str("\r\n");
        Ok(())
    }

    fn validate_folded_header(name: &str, value: &str) -> Result<(), Error> {
        let bytes = value.as_bytes();
        for index in 0..bytes.len() {
            if bytes[index] == b'\r' && (bytes.get(index + 1) != Some(&b'\n') || !matches!(bytes.get(index + 2), Some(b' ' | b'\t'))) {
                return Err(error("InvalidHeader", format!("{name} contains an invalid fold")));
            }
            if bytes[index] == b'\n' && (index == 0 || bytes[index - 1] != b'\r') {
                return Err(error("InvalidHeader", format!("{name} contains a bare newline")));
            }
            if bytes[index] < 32 && !matches!(bytes[index], b'\r' | b'\n' | b'\t') {
                return Err(error("InvalidHeader", format!("{name} contains a control byte")));
            }
        }
        for (index, line) in value.split("\r\n").enumerate() {
            let prefix = if index == 0 { name.len() + 2 } else { 0 };
            if prefix.saturating_add(line.len()).saturating_add(2) > MAX_HEADER_BYTES {
                return Err(error("LimitExceeded", format!("{name} physical header line exceeds {MAX_HEADER_BYTES} bytes")));
            }
        }
        Ok(())
    }

    fn ensure_physical_header_len(name: &str, value_len: usize) -> Result<(), Error> {
        if name.len().saturating_add(2).saturating_add(value_len).saturating_add(2) > MAX_HEADER_BYTES {
            return Err(error("LimitExceeded", format!("{name} physical header line exceeds {MAX_HEADER_BYTES} bytes")));
        }
        Ok(())
    }

    fn ensure_encoded_header_lines(name: &str, value: &str) -> Result<(), Error> {
        if value.is_ascii() {
            ensure_physical_header_len(name, value.len())
        } else {
            if name.len() + 2 + 72 + 2 > MAX_HEADER_BYTES {
                Err(error("LimitExceeded", format!("{name} physical header line exceeds {MAX_HEADER_BYTES} bytes")))
            } else {
                Ok(())
            }
        }
    }

    fn ensure_rendered_address_header(name: &str, addresses: &[Address]) -> Result<(), Error> {
        let rendered = render_addresses(addresses, name);
        validate_folded_header(name, &rendered)
    }

    fn render_addresses(addresses: &[Address], name: &str) -> String {
        let mut out = String::new();
        let mut physical = name.len() + 2;
        for (index, address) in addresses.iter().enumerate() {
            let rendered = render_address(address);
            let separator = if index == 0 { "" } else { ", " };
            if index > 0 && physical + separator.len() + rendered.lines().next().unwrap_or("").len() + 2 > MAX_HEADER_BYTES {
                out.push_str(",\r\n ");
                physical = 1;
            } else {
                out.push_str(separator);
                physical += separator.len();
            }
            out.push_str(&rendered);
            physical = rendered.rsplit("\r\n").next().unwrap_or("").len()
                + if rendered.contains("\r\n") { 0 } else { physical };
        }
        out
    }

    fn render_address(address: &Address) -> String {
        match &address.display {
            Some(display) if display.is_ascii() => format!("{} <{}>", render_ascii_display(display), address.mailbox),
            Some(display) => format!("{} <{}>", encode_header(display), address.mailbox),
            None => address.mailbox.clone(),
        }
    }

    fn render_ascii_display(display: &str) -> String {
        let phrase_safe = display.split(' ').all(|word| !word.is_empty() && word.bytes().all(is_atext));
        if phrase_safe { display.to_string() }
        else { format!("\"{}\"", display.replace('\\', "\\\\").replace('"', "\\\"")) }
    }

    fn rendered_address_len(address: &Address) -> usize {
        match &address.display {
            Some(display) if display.is_ascii() => {
                let safe = display.split(' ').all(|word| !word.is_empty() && word.bytes().all(is_atext));
                let shown = if safe { display.len() } else { 2 + display.bytes().filter(|byte| matches!(byte, b'\\' | b'"')).count() + display.len() };
                shown.saturating_add(address.mailbox.len()).saturating_add(3)
            }
            Some(display) => encoded_header_len(display).saturating_add(address.mailbox.len()).saturating_add(3),
            None => address.mailbox.len(),
        }
    }

    fn encode_header(value: &str) -> String {
        if value.is_ascii() { return value.to_string(); }
        let mut out = String::with_capacity(encoded_header_len(value));
        let mut start = 0usize;
        for (index, ch) in value.char_indices() {
            if index > start && index + ch.len_utf8() - start > 45 {
                if !out.is_empty() { out.push_str("\r\n "); }
                out.push_str("=?UTF-8?B?");
                out.push_str(&base64(&value.as_bytes()[start..index]));
                out.push_str("?=");
                start = index;
            }
        }
        if start < value.len() {
            if !out.is_empty() { out.push_str("\r\n "); }
            out.push_str("=?UTF-8?B?");
            out.push_str(&base64(&value.as_bytes()[start..]));
            out.push_str("?=");
        }
        out
    }

    fn encoded_header_len(value: &str) -> usize {
        if value.is_ascii() { return value.len(); }
        let mut total = 0usize;
        let mut start = 0usize;
        let mut chunks = 0usize;
        for (index, ch) in value.char_indices() {
            if index > start && index + ch.len_utf8() - start > 45 {
                total = total.saturating_add(12).saturating_add(base64_len(index - start));
                start = index;
                chunks += 1;
            }
        }
        if start < value.len() {
            total = total.saturating_add(12).saturating_add(base64_len(value.len() - start));
            chunks += 1;
        }
        total.saturating_add(chunks.saturating_sub(1).saturating_mul(3))
    }

    fn valid_mime(value: &str) -> bool {
        let mut parts = value.split('/');
        let top = parts.next().unwrap_or("");
        let sub = parts.next().unwrap_or("");
        !top.is_empty() && !sub.is_empty() && parts.next().is_none()
            && top.bytes().chain(sub.bytes()).all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'!' | b'#' | b'$' | b'&' | b'-' | b'^' | b'_' | b'.' | b'+'))
    }

    fn boundary(message: &Message, label: &str) -> String {
        let mut state = super::jet_sha256_raw(label.as_bytes());
        let mut index = 1u8;
        let mut absorb = |bytes: &[u8]| {
            let digest = super::jet_sha256_raw(bytes);
            for slot in 0..32 {
                state[slot] = state[slot].wrapping_add(digest[(slot + index as usize) % 32]).rotate_left((index % 7) as u32) ^ index;
            }
            index = index.wrapping_add(1);
        };
        absorb(message.subject.as_bytes());
        absorb(message.text.as_bytes());
        absorb(message.html.as_bytes());
        absorb(message.from.mailbox.as_bytes());
        for address in &message.to { absorb(address.mailbox.as_bytes()); }
        for address in &message.bcc { absorb(address.mailbox.as_bytes()); }
        for attachment in &message.attachments { absorb(&attachment.bytes); }
        let digest = super::jet_sha256_raw(&state);
        let suffix: String = digest[..24].iter().map(|byte| format!("{byte:02x}")).collect();
        format!("jet-{label}-{suffix}")
    }

    fn base64_lines_len(bytes: usize) -> usize {
        let encoded = base64_len(bytes);
        if encoded == 0 { 0 } else { encoded.saturating_add(((encoded + 75) / 76).saturating_sub(1).saturating_mul(2)) }
    }

    fn base64_len(bytes: usize) -> usize {
        bytes.saturating_add(2) / 3 * 4
    }

    fn base64_lines(bytes: &[u8]) -> String {
        let encoded = base64(bytes);
        encoded.as_bytes().chunks(76).map(|line| std::str::from_utf8(line).unwrap()).collect::<Vec<_>>().join("\r\n")
    }

    fn base64(bytes: &[u8]) -> String {
        const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::with_capacity(base64_len(bytes.len()));
        for chunk in bytes.chunks(3) {
            let n = ((chunk[0] as u32) << 16) | ((chunk.get(1).copied().unwrap_or(0) as u32) << 8) | chunk.get(2).copied().unwrap_or(0) as u32;
            out.push(TABLE[(n >> 18) as usize] as char);
            out.push(TABLE[((n >> 12) & 63) as usize] as char);
            out.push(if chunk.len() > 1 { TABLE[((n >> 6) & 63) as usize] as char } else { '=' });
            out.push(if chunk.len() > 2 { TABLE[(n & 63) as usize] as char } else { '=' });
        }
        out
    }

    fn percent_encoded_len(value: &str) -> Result<usize, Error> {
        value.bytes().try_fold(0usize, |total, byte| {
            total.checked_add(if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') { 1 } else { 3 })
                .ok_or_else(|| error("LimitExceeded", "attachment filename encoding length overflow"))
        })
    }

    fn percent_encode(value: &str) -> Result<String, Error> {
        let mut out = String::with_capacity(percent_encoded_len(value)?);
        for byte in value.bytes() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') { out.push(byte as char); }
            else { out.push_str(&format!("%{byte:02X}")); }
        }
        Ok(out)
    }
}
