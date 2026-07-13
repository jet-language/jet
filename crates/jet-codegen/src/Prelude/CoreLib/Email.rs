// D-EMAIL1=A: bounded, dependency-free email address and MIME substrate.
pub mod jet_email {
    pub const MAX_RECIPIENTS: usize = 100;
    pub const MAX_HEADER_BYTES: usize = 998;
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
    }

    fn error(kind: &'static str, message: impl Into<String>) -> Error {
        Error { kind, message: message.into() }
    }

    fn reject_controls(value: &str, what: &str) -> Result<(), Error> {
        if value.chars().any(|ch| ch == '\r' || ch == '\n' || ch == '\0' || (ch.is_control() && ch != '\t')) {
            return Err(error("InvalidHeader", format!("{what} contains a forbidden control character")));
        }
        if value.as_bytes().len() > MAX_HEADER_BYTES {
            return Err(error("LimitExceeded", format!("{what} exceeds {MAX_HEADER_BYTES} bytes")));
        }
        Ok(())
    }

    pub fn address(input: &String) -> Result<Address, Error> {
        reject_controls(input, "email address")?;
        let value = input.trim();
        if value.is_empty() || value.as_bytes().len() > 320 {
            return Err(error("InvalidAddress", "email address must contain 1 to 320 bytes"));
        }
        let (display, mailbox) = if value.ends_with('>') {
            let open = value.rfind('<').ok_or_else(|| error("InvalidAddress", "display address needs `<mailbox>`"))?;
            let shown = value[..open].trim().trim_matches('"').trim();
            if shown.is_empty() {
                return Err(error("InvalidAddress", "display name cannot be empty"));
            }
            (Some(shown.to_string()), value[open + 1..value.len() - 1].trim())
        } else {
            (None, value)
        };
        if mailbox.chars().any(|ch| ch.is_whitespace() || ch.is_control() || matches!(ch, '<' | '>' | ',' | ';')) {
            return Err(error("InvalidAddress", "mailbox contains an invalid character"));
        }
        let mut pieces = mailbox.split('@');
        let local = pieces.next().unwrap_or("");
        let domain = pieces.next().unwrap_or("");
        if local.is_empty() || domain.is_empty() || pieces.next().is_some() || local.len() > 64 || domain.len() > 255 {
            return Err(error("InvalidAddress", "mailbox needs one non-empty local part and domain"));
        }
        if local.starts_with('.') || local.ends_with('.') || local.contains("..")
            || domain.starts_with('.') || domain.ends_with('.') || domain.contains("..")
            || domain.split('.').any(|label| label.is_empty() || label.starts_with('-') || label.ends_with('-'))
        {
            return Err(error("InvalidAddress", "mailbox has an invalid dot or domain-label boundary"));
        }
        Ok(Address { display, mailbox: mailbox.to_string() })
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
        if to.is_empty() {
            return Err(error("InvalidMessage", "message needs at least one visible recipient"));
        }
        if to.len().saturating_add(bcc.len()) > MAX_RECIPIENTS {
            return Err(error("LimitExceeded", format!("message exceeds {MAX_RECIPIENTS} recipients")));
        }
        if text.is_empty() && html.is_empty() {
            return Err(error("InvalidMessage", "message needs text or HTML content"));
        }
        let attachment_bytes = attachments.iter().try_fold(0usize, |total, item| {
            total.checked_add(item.bytes.len()).ok_or_else(|| error("LimitExceeded", "attachment bytes overflow"))
        })?;
        if attachment_bytes > MAX_MESSAGE_BYTES {
            return Err(error("LimitExceeded", format!("attachments exceed {MAX_MESSAGE_BYTES} total bytes")));
        }
        Ok(Message {
            from: from.clone(), to: to.clone(), bcc: bcc.clone(), subject: subject.clone(),
            text: text.clone(), html: html.clone(), attachments: attachments.clone(),
        })
    }

    pub fn serialize(message: &Message) -> Result<Vec<u8>, Error> {
        let mixed = boundary(message, "mixed");
        let alternative = boundary(message, "alternative");
        let mut out = String::new();
        header(&mut out, "From", &render_address(&message.from))?;
        header(&mut out, "To", &message.to.iter().map(render_address).collect::<Vec<_>>().join(", "))?;
        header(&mut out, "Subject", &encode_header(&message.subject))?;
        out.push_str("MIME-Version: 1.0\r\n");
        if message.attachments.is_empty() {
            render_body(&mut out, message, &alternative);
        } else {
            out.push_str(&format!("Content-Type: multipart/mixed; boundary=\"{mixed}\"\r\n\r\n"));
            out.push_str(&format!("--{mixed}\r\n"));
            render_body(&mut out, message, &alternative);
            for item in &message.attachments {
                out.push_str(&format!("\r\n--{mixed}\r\n"));
                out.push_str(&format!("Content-Type: {}\r\n", item.mime));
                out.push_str("Content-Transfer-Encoding: base64\r\n");
                out.push_str(&format!("Content-Disposition: attachment; filename*=UTF-8''{}\r\n\r\n", percent_encode(&item.filename)));
                out.push_str(&base64_lines(&item.bytes));
            }
            out.push_str(&format!("\r\n--{mixed}--\r\n"));
        }
        if out.len() > MAX_MESSAGE_BYTES {
            return Err(error("LimitExceeded", format!("serialized message exceeds {MAX_MESSAGE_BYTES} bytes")));
        }
        Ok(out.into_bytes())
    }

    fn render_body(out: &mut String, message: &Message, alternative: &str) {
        if message.html.is_empty() {
            text_part(out, "text/plain", &message.text);
        } else if message.text.is_empty() {
            text_part(out, "text/html", &message.html);
        } else {
            out.push_str(&format!("Content-Type: multipart/alternative; boundary=\"{alternative}\"\r\n\r\n"));
            out.push_str(&format!("--{alternative}\r\n"));
            text_part(out, "text/plain", &message.text);
            out.push_str(&format!("\r\n--{alternative}\r\n"));
            text_part(out, "text/html", &message.html);
            out.push_str(&format!("\r\n--{alternative}--\r\n"));
        }
    }

    fn text_part(out: &mut String, mime: &str, body: &str) {
        out.push_str(&format!("Content-Type: {mime}; charset=utf-8\r\n"));
        out.push_str("Content-Transfer-Encoding: base64\r\n\r\n");
        out.push_str(&base64_lines(body.as_bytes()));
    }

    fn header(out: &mut String, name: &str, value: &str) -> Result<(), Error> {
        reject_controls(value, name)?;
        out.push_str(name);
        out.push_str(": ");
        out.push_str(value);
        out.push_str("\r\n");
        Ok(())
    }

    fn render_address(address: &Address) -> String {
        match &address.display {
            Some(display) => format!("{} <{}>", encode_header(display), address.mailbox),
            None => address.mailbox.clone(),
        }
    }

    fn encode_header(value: &str) -> String {
        if value.is_ascii() { value.to_string() }
        else { format!("=?UTF-8?B?{}?=", base64(value.as_bytes())) }
    }

    fn valid_mime(value: &str) -> bool {
        let mut parts = value.split('/');
        let top = parts.next().unwrap_or("");
        let sub = parts.next().unwrap_or("");
        !top.is_empty() && !sub.is_empty() && parts.next().is_none()
            && top.bytes().chain(sub.bytes()).all(|b| b.is_ascii_alphanumeric() || matches!(b, b'!' | b'#' | b'$' | b'&' | b'-' | b'^' | b'_' | b'.' | b'+'))
    }

    fn boundary(message: &Message, label: &str) -> String {
        let mut source = Vec::new();
        for byte in label.bytes().chain(message.subject.bytes()).chain(message.text.bytes()).chain(message.html.bytes())
            .chain(message.from.mailbox.bytes()).chain(message.to.iter().flat_map(|v| v.mailbox.bytes()))
            .chain(message.bcc.iter().flat_map(|v| v.mailbox.bytes())).chain(message.attachments.iter().flat_map(|v| v.bytes.iter().copied()))
        {
            source.push(byte);
        }
        let digest = super::jet_sha256_raw(&source);
        let suffix: String = digest[..24].iter().map(|byte| format!("{byte:02x}")).collect();
        format!("jet-{label}-{suffix}")
    }

    fn base64_lines(bytes: &[u8]) -> String {
        let encoded = base64(bytes);
        encoded.as_bytes().chunks(76).map(|line| std::str::from_utf8(line).unwrap()).collect::<Vec<_>>().join("\r\n")
    }

    fn base64(bytes: &[u8]) -> String {
        const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
        for chunk in bytes.chunks(3) {
            let n = ((chunk[0] as u32) << 16) | ((chunk.get(1).copied().unwrap_or(0) as u32) << 8) | chunk.get(2).copied().unwrap_or(0) as u32;
            out.push(TABLE[(n >> 18) as usize] as char);
            out.push(TABLE[((n >> 12) & 63) as usize] as char);
            out.push(if chunk.len() > 1 { TABLE[((n >> 6) & 63) as usize] as char } else { '=' });
            out.push(if chunk.len() > 2 { TABLE[(n & 63) as usize] as char } else { '=' });
        }
        out
    }

    fn percent_encode(value: &str) -> String {
        let mut out = String::new();
        for byte in value.bytes() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') { out.push(byte as char); }
            else { out.push_str(&format!("%{byte:02X}")); }
        }
        out
    }
}
