// D-EMAIL1=A: bounded, dependency-free email address and MIME substrate.
pub mod jet_email {
    // The one outcome carrier: from the flat Prelude under AOT, from the host
    // module when another tier includes this file.
    #[allow(unused_imports)]
    use super::{JetAbsent, JetOutcome};
    pub const MAX_RECIPIENTS: usize = 100;
    pub const MAX_ATTACHMENTS: usize = 64;
    pub const MAX_HEADER_BYTES: usize = 998;
    pub const MAX_HEADER_VALUE_BYTES: usize = 64 * 1024;
    pub const MAX_BODY_BYTES: usize = 1024 * 1024;
    pub const MAX_ATTACHMENT_BYTES: usize = 25 * 1024 * 1024;
    pub const MAX_MESSAGE_BYTES: usize = 32 * 1024 * 1024;

    #[derive(Clone, Debug, PartialEq)]
    pub enum Error {
        Configuration { operation: String, server: Option<String>, code: Option<i64>, reason: String },
        DNS { operation: String, server: Option<String>, code: Option<i64>, reason: String },
        Connect { operation: String, server: Option<String>, code: Option<i64>, reason: String },
        TLS { operation: String, server: Option<String>, code: Option<i64>, reason: String },
        Auth { operation: String, server: Option<String>, code: Option<i64>, reason: String },
        Protocol { operation: String, server: Option<String>, code: Option<i64>, reason: String },
        Rejected { operation: String, server: Option<String>, code: Option<i64>, reason: String },
        Transient { operation: String, server: Option<String>, code: Option<i64>, reason: String },
        TimedOut { operation: String, server: Option<String>, code: Option<i64>, reason: String },
        Cancelled { operation: String, server: Option<String>, code: Option<i64>, reason: String },
        DeliveryUnknown { operation: String, server: Option<String>, code: Option<i64>, reason: String },
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct Address {
        pub(crate) display: Option<String>,
        pub(crate) mailbox: String,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct Attachment {
        pub(crate) filename: String,
        pub(crate) mime: String,
        pub(crate) bytes: Vec<u8>,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct Message {
        pub(crate) from: Address,
        pub(crate) to: Vec<Address>,
        pub(crate) bcc: Vec<Address>,
        pub(crate) subject: String,
        pub(crate) text: String,
        pub(crate) html: String,
        pub(crate) attachments: Vec<Attachment>,
        pub(crate) envelope: Envelope,
        pub(crate) wire_upper: usize,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct Envelope {
        pub from: Address,
        pub recipients: Vec<Address>,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub enum SMTPSecurity { StartTls, TLS }

    #[derive(Clone, Debug, PartialEq)]
    pub enum RecipientPolicy { RequireAll, DeliverAccepted }

    #[derive(Clone, Debug, PartialEq)]
    pub struct Limits {
        pub max_reply_line_bytes: i64,
        pub max_reply_lines: i64,
        pub max_capabilities: i64,
        pub max_recipients: i64,
        pub max_message_bytes: i64,
        pub max_auth_challenge_bytes: i64,
    }

    impl Limits {
        pub fn safe() -> Limits {
            Limits {
                max_reply_line_bytes: 512,
                max_reply_lines: 100,
                max_capabilities: 100,
                max_recipients: 100,
                max_message_bytes: 33_554_432,
                max_auth_challenge_bytes: 4096,
            }
        }

        pub fn validate(&self) -> Result<(), Error> {
            for (field, value, min, max) in [
                ("max_reply_line_bytes", self.max_reply_line_bytes, 64, 65_536),
                ("max_reply_lines", self.max_reply_lines, 1, 1000),
                ("max_capabilities", self.max_capabilities, 1, 1000),
                ("max_recipients", self.max_recipients, 1, 10_000),
                ("max_message_bytes", self.max_message_bytes, 1, 1_073_741_824),
                ("max_auth_challenge_bytes", self.max_auth_challenge_bytes, 1, 65_536),
            ] {
                if value < min || value > max {
                    return Err(error("smtp", format!(
                        "{field} must be between {min} and {max}; got {value}"
                    )));
                }
            }
            Ok(())
        }
    }

    // Hidden Rust generic preserves Jet's single canonical Secret type. The
    // byte default keeps direct Prelude consumers well-typed when no secret
    // value is present; runtime entry points still name Vec<u8> explicitly.
    pub enum SMTPAuth<S = Vec<u8>> {
        None,
        Password { username: String, password: S },
    }

    pub struct DkimConfig<S = Vec<u8>> {
        pub domain: String,
        pub selector: String,
        pub private_key: S,
        pub signed_headers: Vec<String>,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub enum TLSTrust {
        System,
        SystemPlusCa { pem: Vec<u8> },
    }

    pub struct SMTPConfig<S = Vec<u8>> {
        pub host: String,
        pub port: i64,
        pub security: SMTPSecurity,
        pub auth: SMTPAuth<S>,
        pub recipient_policy: RecipientPolicy,
        pub trust: TLSTrust,
        pub limits: Limits,
        pub dkim: JetOutcome<DkimConfig<S>, JetAbsent>,
    }

    #[derive(Clone, Copy)]
    pub struct RuntimeFns {
        pub tls_begin: fn(std::net::TcpStream, &String) -> Result<i64, String>,
        pub tls_begin_ca: fn(std::net::TcpStream, &String, &Vec<u8>) -> Result<i64, String>,
        pub tls_handshake_step: fn(i64) -> Result<bool, String>,
        pub tls_set_poll_timeout: fn(i64, i64) -> Result<(), String>,
        pub tls_read: fn(i64, i64) -> Result<Vec<u8>, String>,
        pub tls_write_all: fn(i64, &Vec<u8>) -> Result<(), String>,
        pub tls_close: fn(i64) -> Result<(), String>,
        pub wipe: fn(&mut Vec<u8>),
        pub sha256: fn(&[u8]) -> [u8; 32],
        pub ed25519_sign: fn(&Vec<u8>, &[u8]) -> Result<Vec<u8>, String>,
        pub cancelled: fn() -> bool,
        pub remaining_ms: fn() -> Option<i64>,
        pub accepted_at: fn() -> String,
    }

    pub fn runtime_now() -> String {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_secs().to_string()).unwrap_or_else(|_| "0".to_string())
    }

    pub struct Mailer {
        config: SMTPConfig<Vec<u8>>,
        runtime: RuntimeFns,
    }

    impl Drop for Mailer {
        fn drop(&mut self) {
            if let SMTPAuth::Password { password, .. } = &mut self.config.auth {
                (self.runtime.wipe)(password);
            }
            if let Ok(dkim) = &mut self.config.dkim {
                (self.runtime.wipe)(&mut dkim.private_key);
            }
        }
    }

    pub fn smtp<S>(
        config: &SMTPConfig<S>,
        extract: fn(&S) -> Vec<u8>,
        runtime: RuntimeFns,
    ) -> Result<Mailer, Error> {
        let auth = match &config.auth {
            SMTPAuth::None => SMTPAuth::<Vec<u8>>::None,
            SMTPAuth::Password { username, password } => SMTPAuth::<Vec<u8>>::Password {
                username: username.clone(),
                password: extract(password),
            },
        };
        let dkim = config.dkim.as_ref().map(|dkim| DkimConfig::<Vec<u8>> {
            domain: dkim.domain.clone(),
            selector: dkim.selector.clone(),
            private_key: extract(&dkim.private_key),
            signed_headers: dkim.signed_headers.clone(),
        }).map_err(|_| JetAbsent);
        smtp_bytes(SMTPConfig::<Vec<u8>> {
            host: config.host.clone(), port: config.port, security: config.security.clone(), auth,
            recipient_policy: config.recipient_policy.clone(), trust: config.trust.clone(),
            limits: config.limits.clone(), dkim,
        }, runtime)
    }

    fn smtp_bytes(mut config: SMTPConfig<Vec<u8>>, runtime: RuntimeFns) -> Result<Mailer, Error> {
        if let Err(error) = validate_smtp_config(&config) {
            wipe_config_secrets(&mut config, runtime);
            return Err(error);
        }
        if config.dkim.as_ref().is_ok_and(|dkim| dkim.private_key.len() != 32) {
            wipe_config_secrets(&mut config, runtime);
            return Err(error("dkim", "private_key must contain exactly 32 bytes"));
        }
        if let SMTPAuth::Password { password, .. } = &config.auth {
            if std::str::from_utf8(password).is_err() {
                wipe_config_secrets(&mut config, runtime);
                return Err(error("auth", "SMTP password must be UTF-8"));
            }
        }
        Ok(Mailer { config, runtime })
    }

    fn wipe_config_secrets(config: &mut SMTPConfig<Vec<u8>>, runtime: RuntimeFns) {
        if let SMTPAuth::Password { password, .. } = &mut config.auth { (runtime.wipe)(password); }
        if let Ok(dkim) = &mut config.dkim { (runtime.wipe)(&mut dkim.private_key); }
    }

    pub fn smtp_from_env(runtime: RuntimeFns) -> Result<Mailer, Error> {
        let host = std::env::var("SMTP_HOST")
            .map_err(|_| error("smtp_from_env", "SMTP_HOST is required"))?;
        let security_text = std::env::var("SMTP_SECURITY").unwrap_or_else(|_| "starttls".to_string());
        let security = match security_text.to_ascii_lowercase().as_str() {
            "starttls" => SMTPSecurity::StartTls,
            "tls" => SMTPSecurity::TLS,
            _ => return Err(error("smtp_from_env", "SMTP_SECURITY must be `starttls` or `tls`")),
        };
        let default_port = if security == SMTPSecurity::TLS { 465 } else { 587 };
        let port = match std::env::var("SMTP_PORT") {
            Ok(value) => value.parse::<i64>().map_err(|_| error("smtp_from_env", "SMTP_PORT must be an integer"))?,
            Err(_) => default_port,
        };
        let recipient_policy = match std::env::var("SMTP_RECIPIENT_POLICY")
            .unwrap_or_else(|_| "require_all".to_string()).to_ascii_lowercase().as_str() {
            "require_all" => RecipientPolicy::RequireAll,
            "deliver_accepted" => RecipientPolicy::DeliverAccepted,
            _ => return Err(error("smtp_from_env", "SMTP_RECIPIENT_POLICY must be `require_all` or `deliver_accepted`")),
        };
        let trust = match std::env::var("SMTP_CA_PEM") {
            Ok(mut pem) => TLSTrust::SystemPlusCa { pem: std::mem::take(&mut pem).into_bytes() },
            Err(_) => TLSTrust::System,
        };
        let username = std::env::var("SMTP_USERNAME").ok();
        let password = std::env::var("SMTP_PASSWORD").ok();
        let mut auth = match (username, password) {
            (None, None) => SMTPAuth::<Vec<u8>>::None,
            (Some(username), Some(mut password)) => {
                let bytes = std::mem::take(&mut password).into_bytes();
                SMTPAuth::<Vec<u8>>::Password { username, password: bytes }
            }
            (None, Some(mut password)) => {
                let mut bytes = std::mem::take(&mut password).into_bytes();
                (runtime.wipe)(&mut bytes);
                return Err(error("smtp_from_env", "SMTP_USERNAME and SMTP_PASSWORD must be set together"));
            }
            (Some(_), None) => return Err(error("smtp_from_env", "SMTP_USERNAME and SMTP_PASSWORD must be set together")),
        };
        let domain = std::env::var("SMTP_DKIM_DOMAIN").ok();
        let selector = std::env::var("SMTP_DKIM_SELECTOR").ok();
        let private_key = std::env::var("SMTP_DKIM_PRIVATE_KEY_BASE64").ok();
        let signed_headers_env = std::env::var("SMTP_DKIM_SIGNED_HEADERS").ok();
        let dkim = match (domain, selector, private_key) {
            (None, None, None) if signed_headers_env.is_none() => Err(JetAbsent),
            (Some(domain), Some(selector), Some(private_key)) => {
                let mut encoded = private_key.into_bytes();
                let decoded = decode_dkim_base64(&encoded, runtime.wipe);
                (runtime.wipe)(&mut encoded);
                let private_key = match decoded {
                    Ok(key) => key,
                    Err(error) => { wipe_auth(&mut auth, runtime); return Err(error); }
                };
                let signed_headers = match signed_headers_env {
                    Some(value) => value.split(',').map(|name| name.trim().to_string()).collect(),
                    None => default_dkim_headers(),
                };
                Ok(DkimConfig::<Vec<u8>> { domain, selector, private_key, signed_headers })
            }
            (_, _, Some(private_key)) => {
                let mut encoded = private_key.into_bytes();
                (runtime.wipe)(&mut encoded);
                wipe_auth(&mut auth, runtime);
                return Err(error("smtp_from_env", "SMTP_DKIM_DOMAIN, SMTP_DKIM_SELECTOR, and SMTP_DKIM_PRIVATE_KEY_BASE64 must be set together"));
            }
            _ => {
                wipe_auth(&mut auth, runtime);
                return Err(error("smtp_from_env", "SMTP_DKIM_DOMAIN, SMTP_DKIM_SELECTOR, and SMTP_DKIM_PRIVATE_KEY_BASE64 must be set together"));
            }
        };
        smtp_bytes(SMTPConfig::<Vec<u8>> { host, port, security, auth, recipient_policy, trust,
            limits: Limits::safe(), dkim }, runtime)
    }

    fn wipe_auth(auth: &mut SMTPAuth<Vec<u8>>, runtime: RuntimeFns) {
        if let SMTPAuth::Password { password, .. } = auth { (runtime.wipe)(password); }
    }

    pub fn validate_smtp_config<S>(config: &SMTPConfig<S>) -> Result<(), Error> {
        config.limits.validate()?;
        if config.host.is_empty() || config.host.len() > 253 || !config.host.is_ascii()
            || config.host.bytes().any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
        {
            return Err(error("smtp", "host must contain 1 to 253 non-whitespace ASCII bytes"));
        }
        if !(1..=65_535).contains(&config.port) {
            return Err(error("smtp", "port must be between 1 and 65535"));
        }
        if config.port == 587 && config.security != SMTPSecurity::StartTls {
            return Err(error("smtp", "port 587 requires verified STARTTLS"));
        }
        if config.port == 465 && config.security != SMTPSecurity::TLS {
            return Err(error("smtp", "port 465 requires TLS from connect"));
        }
        if let SMTPAuth::Password { username, .. } = &config.auth {
            if username.is_empty() || username.len() > 512
                || username.bytes().any(|byte| byte.is_ascii_control())
            {
                return Err(error("smtp", "SMTP username must contain 1 to 512 bytes without controls"));
            }
        }
        if let TLSTrust::SystemPlusCa { pem } = &config.trust {
            validate_ca_pem(pem)?;
        }
        if let Ok(dkim) = &config.dkim { validate_dkim_config(dkim)?; }
        Ok(())
    }

    fn default_dkim_headers() -> Vec<String> {
        ["from", "to", "subject", "mime-version", "content-type"]
            .iter().map(|name| name.to_string()).collect()
    }

    fn valid_dns_name(value: &str) -> bool {
        !value.is_empty() && value.len() <= 253 && value.is_ascii()
            && value.split('.').all(|label| !label.is_empty() && label.len() <= 63
                && !label.starts_with('-') && !label.ends_with('-')
                && label.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'))
    }

    fn validate_dkim_config<S>(dkim: &DkimConfig<S>) -> Result<(), Error> {
        if !valid_dns_name(&dkim.domain) {
            return Err(error("dkim", "domain must be a valid ASCII DNS name"));
        }
        if dkim.selector.contains('.') || !valid_dns_name(&dkim.selector) {
            return Err(error("dkim", "selector must be one valid ASCII DNS label"));
        }
        if dkim.signed_headers.is_empty() {
            return Err(error("dkim", "signed_headers must not be empty"));
        }
        if dkim.signed_headers.len() > 64 {
            return Err(error("dkim", "signed_headers must contain at most 64 names"));
        }
        let mut normalized = Vec::new();
        for name in &dkim.signed_headers {
            let lower = name.to_ascii_lowercase();
            if name.is_empty() || !name.is_ascii()
                || !name.bytes().all(|byte| (33..=126).contains(&byte) && byte != b':')
            {
                return Err(error("dkim", "signed_headers must contain valid header names"));
            }
            if matches!(lower.as_str(), "received" | "return-path" | "dkim-signature"
                | "authentication-results" | "arc-authentication-results"
                | "arc-message-signature" | "arc-seal")
            {
                return Err(error("dkim", format!("signed_headers contains forbidden hop header `{lower}`")));
            }
            if normalized.contains(&lower) {
                return Err(error("dkim", format!("signed_headers contains duplicate `{lower}`")));
            }
            normalized.push(lower);
        }
        if !normalized.iter().any(|name| name == "from") {
            return Err(error("dkim", "signed_headers must include `from`"));
        }
        let list_bytes = normalized.iter().map(String::len).sum::<usize>()
            .saturating_add(normalized.len().saturating_sub(1));
        if list_bytes + " h=;\r\n".len() > MAX_HEADER_BYTES {
            return Err(error("dkim", "signed_headers exceeds the DKIM header line bound"));
        }
        Ok(())
    }

    fn validate_ca_pem(pem: &[u8]) -> Result<(), Error> {
        if pem.is_empty() {
            return Err(error("smtp", "custom CA PEM must not be empty"));
        }
        let text = std::str::from_utf8(pem)
            .map_err(|_| error("smtp", "custom CA PEM must be UTF-8 text"))?;
        let begin = "-----BEGIN CERTIFICATE-----";
        let end = "-----END CERTIFICATE-----";
        let mut rest = text;
        let mut certificates = 0usize;
        while let Some(start) = rest.find(begin) {
            if !rest[..start].trim().is_empty() {
                return Err(error("smtp", "custom CA PEM contains data outside certificate blocks"));
            }
            rest = &rest[start + begin.len()..];
            let stop = rest.find(end)
                .ok_or_else(|| error("smtp", "custom CA PEM has an unterminated certificate"))?;
            let der = decode_pem_base64(&rest[..stop])?;
            if der.len() < 4 || der[0] != 0x30 {
                return Err(error("smtp", "custom CA PEM does not contain an X.509 DER certificate"));
            }
            certificates = certificates.checked_add(1)
                .ok_or_else(|| error("smtp", "custom CA certificate count overflow"))?;
            rest = &rest[stop + end.len()..];
        }
        if certificates == 0 || !rest.trim().is_empty() {
            return Err(error("smtp", "custom CA PEM must contain only certificate blocks"));
        }
        Ok(())
    }

    fn decode_pem_base64(text: &str) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        let mut quartet = [0u8; 4];
        let mut used = 0usize;
        let mut padded = false;
        for byte in text.bytes().filter(|byte| !byte.is_ascii_whitespace()) {
            if padded {
                return Err(error("smtp", "custom CA PEM has data after base64 padding"));
            }
            quartet[used] = byte;
            used += 1;
            if used == 4 {
                let mut values = [0u8; 4];
                let mut pads = 0usize;
                for index in 0..4 {
                    if quartet[index] == b'=' {
                        pads += 1;
                    } else {
                        if pads != 0 {
                            return Err(error("smtp", "custom CA PEM has invalid base64 padding"));
                        }
                        values[index] = base64_value(quartet[index])
                            .ok_or_else(|| error("smtp", "custom CA PEM contains invalid base64"))?;
                    }
                }
                if pads > 2 {
                    return Err(error("smtp", "custom CA PEM has invalid base64 padding"));
                }
                out.push(values[0] << 2 | values[1] >> 4);
                if pads < 2 { out.push(values[1] << 4 | values[2] >> 2); }
                if pads == 0 { out.push(values[2] << 6 | values[3]); }
                padded = pads != 0;
                used = 0;
            }
        }
        if used != 0 || out.is_empty() {
            return Err(error("smtp", "custom CA PEM contains incomplete base64"));
        }
        Ok(out)
    }

    fn base64_value(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }

    fn decode_dkim_base64(text: &[u8], wipe: fn(&mut Vec<u8>)) -> Result<Vec<u8>, Error> {
        if text.is_empty() || text.len() % 4 != 0 {
            return Err(error("smtp_from_env", "SMTP_DKIM_PRIVATE_KEY_BASE64 must be valid base64"));
        }
        let mut out = Vec::new();
        for (chunk_index, chunk) in text.chunks_exact(4).enumerate() {
            let last = chunk_index + 1 == text.len() / 4;
            let pads = usize::from(chunk[3] == b'=') + usize::from(chunk[2] == b'=');
            if pads > 2 || (!last && pads != 0) || (chunk[2] == b'=' && chunk[3] != b'=') {
                wipe(&mut out);
                return Err(error("smtp_from_env", "SMTP_DKIM_PRIVATE_KEY_BASE64 must be valid base64"));
            }
            let a = base64_value(chunk[0]);
            let b = base64_value(chunk[1]);
            let c = if chunk[2] == b'=' { Some(0) } else { base64_value(chunk[2]) };
            let d = if chunk[3] == b'=' { Some(0) } else { base64_value(chunk[3]) };
            let (Some(a), Some(b), Some(c), Some(d)) = (a, b, c, d) else {
                wipe(&mut out);
                return Err(error("smtp_from_env", "SMTP_DKIM_PRIVATE_KEY_BASE64 must be valid base64"));
            };
            if (pads == 2 && b & 0x0f != 0) || (pads == 1 && c & 0x03 != 0) {
                wipe(&mut out);
                return Err(error("smtp_from_env", "SMTP_DKIM_PRIVATE_KEY_BASE64 must be valid base64"));
            }
            out.push(a << 2 | b >> 4);
            if pads < 2 { out.push(b << 4 | c >> 2); }
            if pads == 0 { out.push(c << 6 | d); }
        }
        if out.len() != 32 {
            wipe(&mut out);
            return Err(error("smtp_from_env", "SMTP_DKIM_PRIVATE_KEY_BASE64 must decode to exactly 32 bytes"));
        }
        Ok(out)
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct SMTPReply { pub code: i64, pub lines: Vec<String> }

    // Byte-at-a-time CRLF reading keeps the configured line bound prospective.
    pub fn read_smtp_reply<R: std::io::Read>(reader: &mut R, limits: &Limits) -> Result<SMTPReply, Error> {
        limits.validate()?;
        let mut lines = Vec::new();
        let mut expected = None;
        loop {
            if lines.len() == limits.max_reply_lines as usize {
                return Err(protocol_error("reply exceeds max_reply_lines"));
            }
            let mut line = Vec::new();
            loop {
                let mut byte = [0u8; 1];
                match reader.read(&mut byte) {
                    Ok(0) => return Err(protocol_error("connection closed inside SMTP reply")),
                    Ok(_) => {
                        if line.len() == limits.max_reply_line_bytes as usize {
                            return Err(protocol_error("reply line exceeds max_reply_line_bytes"));
                        }
                        line.push(byte[0]);
                        if line.ends_with(b"\r\n") { break; }
                    }
                    Err(err) => return Err(protocol_error(format!("failed reading SMTP reply: {err}"))),
                }
            }
            if line.len() < 5 || !line[..3].iter().all(u8::is_ascii_digit)
                || !matches!(line[3], b'-' | b' ')
            {
                return Err(protocol_error("SMTP reply must start with three digits and space or hyphen"));
            }
            let code = ((line[0] - b'0') as i64) * 100
                + ((line[1] - b'0') as i64) * 10 + (line[2] - b'0') as i64;
            if expected.replace(code).is_some_and(|value| value != code) {
                return Err(protocol_error("multiline SMTP reply changed response code"));
            }
            let continued = line[3] == b'-';
            let body = std::str::from_utf8(&line[4..line.len() - 2])
                .map_err(|_| protocol_error("SMTP reply is not UTF-8"))?;
            lines.push(body.to_string());
            if !continued { return Ok(SMTPReply { code, lines }); }
        }
    }

    fn protocol_error(reason: impl Into<String>) -> Error {
        Error::Protocol { operation: "smtp_reply".to_string(), server: None, code: None, reason: reason.into() }
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct SMTPCapabilities {
        pub names: Vec<String>,
        auth_mechanisms: Vec<String>,
    }

    pub fn smtp_capabilities(reply: &SMTPReply, limits: &Limits) -> Result<SMTPCapabilities, Error> {
        limits.validate()?;
        if reply.code != 250 { return Err(protocol_error("EHLO requires a 250 reply")); }
        let mut names = Vec::new();
        let mut auth_mechanisms = Vec::new();
        for line in reply.lines.iter().skip(1) {
            let mut words = line.split_ascii_whitespace();
            let name = words.next().unwrap_or("");
            if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                return Err(protocol_error("EHLO capability name is invalid"));
            }
            let name = name.to_ascii_uppercase();
            if !names.contains(&name) {
                if names.len() == limits.max_capabilities as usize {
                    return Err(protocol_error("EHLO exceeds max_capabilities"));
                }
                names.push(name.clone());
            }
            if name == "AUTH" {
                for mechanism in words {
                    if !mechanism.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-') {
                        return Err(protocol_error("EHLO AUTH mechanism is invalid"));
                    }
                    let mechanism = mechanism.to_ascii_uppercase();
                    if !auth_mechanisms.contains(&mechanism) {
                        auth_mechanisms.push(mechanism);
                    }
                }
            }
        }
        Ok(SMTPCapabilities { names, auth_mechanisms })
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct SMTPState { greeted: bool, ehlo: bool, verified_tls: bool, authenticated: bool }

    impl SMTPState {
        pub fn new() -> SMTPState {
            SMTPState { greeted: false, ehlo: false, verified_tls: false, authenticated: false }
        }
        pub fn greeting(&mut self, reply: &SMTPReply) -> Result<(), Error> {
            if self.greeted || reply.code != 220 { return Err(protocol_error("expected one SMTP 220 greeting")); }
            self.greeted = true; Ok(())
        }
        pub fn ehlo(&mut self, reply: &SMTPReply, limits: &Limits) -> Result<SMTPCapabilities, Error> {
            if !self.greeted { return Err(protocol_error("EHLO cannot precede greeting")); }
            let caps = smtp_capabilities(reply, limits)?;
            self.ehlo = true; Ok(caps)
        }
        pub fn start_tls(&mut self, caps: &SMTPCapabilities) -> Result<(), Error> {
            if !self.ehlo || self.verified_tls || !caps.names.iter().any(|name| name == "STARTTLS") {
                return Err(Error::TLS { operation: "starttls".to_string(), server: None, code: None,
                    reason: "verified STARTTLS requires an advertised capability after EHLO".to_string() });
            }
            self.ehlo = false; Ok(())
        }
        pub fn verified_tls(&mut self) { self.verified_tls = true; }
        pub fn authenticate(&mut self, auth: &str, challenge_bytes: usize, limits: &Limits) -> Result<(), Error> {
            if !self.verified_tls || !self.ehlo {
                return Err(Error::Auth { operation: "auth".to_string(), server: None, code: None,
                    reason: "password authentication requires verified TLS and post-TLS EHLO".to_string() });
            }
            if challenge_bytes > limits.max_auth_challenge_bytes as usize {
                return Err(Error::Auth { operation: "auth".to_string(), server: None, code: None,
                    reason: "authentication challenge exceeds max_auth_challenge_bytes".to_string() });
            }
            if auth != "PLAIN" && auth != "LOGIN" {
                return Err(Error::Auth { operation: "auth".to_string(), server: None, code: None,
                    reason: "relay does not offer supported password authentication".to_string() });
            }
            self.authenticated = true; Ok(())
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct RecipientReport {
        pub address: Address,
        pub accepted: bool,
        pub code: i64,
        pub message: String,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct SendReport {
        pub server: String,
        pub accepted: Vec<RecipientReport>,
        pub rejected: Vec<RecipientReport>,
        pub response_code: i64,
        pub response: String,
        pub accepted_at: String,
    }

    // Hidden transport seam for D-EMAIL1's one synchronous send mechanism.
    // Concrete runtime adapters own TCP, verified rustls, scheduler waits, and
    // ambient context. This engine owns SMTP ordering and never retries.
    pub trait SMTPTransport: std::io::Read + std::io::Write {
        fn verified_tls(&self) -> bool;
        fn start_tls(&mut self, server: &str, trust: &TLSTrust) -> Result<(), String>;
        fn close(&mut self);
        fn take_stop(&mut self) -> Option<SMTPStop> { None }
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum SMTPStop { Cancelled, TimedOut }

    pub trait SMTPControl {
        fn checkpoint(&self, operation: &str) -> Result<(), SMTPStop>;
        fn accepted_at(&self) -> String;
        fn wipe(&self, bytes: &mut Vec<u8>) { bytes.fill(0); }
    }

    pub struct NoopSmtpControl;

    impl SMTPControl for NoopSmtpControl {
        fn checkpoint(&self, _operation: &str) -> Result<(), SMTPStop> { Ok(()) }

        fn accepted_at(&self) -> String {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|value| value.as_secs().to_string())
                .unwrap_or_else(|_| "0".to_string())
        }
    }

    #[derive(Clone, Copy)]
    struct AmbientSmtpControl(RuntimeFns);

    impl SMTPControl for AmbientSmtpControl {
        fn checkpoint(&self, _operation: &str) -> Result<(), SMTPStop> {
            if (self.0.cancelled)() { return Err(SMTPStop::Cancelled); }
            if matches!((self.0.remaining_ms)(), Some(value) if value <= 0) {
                return Err(SMTPStop::TimedOut);
            }
            Ok(())
        }

        fn accepted_at(&self) -> String { (self.0.accepted_at)() }

        fn wipe(&self, bytes: &mut Vec<u8>) { (self.0.wipe)(bytes); }
    }

    enum RuntimeStream {
        Plain(Option<std::net::TcpStream>),
        TLS(i64),
        Closed,
    }

    struct RuntimeTransport {
        stream: RuntimeStream,
        runtime: RuntimeFns,
        control: AmbientSmtpControl,
        stopped: Option<SMTPStop>,
    }

    impl RuntimeTransport {
        fn connect(config: &SMTPConfig<Vec<u8>>, runtime: RuntimeFns) -> Result<Self, Error> {
            use std::net::ToSocketAddrs;
            let control = AmbientSmtpControl(runtime);
            control.checkpoint("connect").map_err(|stop| stop_error(stop, "connect", &config.host, false))?;
            let addresses = (config.host.as_str(), config.port as u16).to_socket_addrs()
                .map_err(|reason| smtp_error(ErrorKind::DNS, "dns", &config.host, None, format!("SMTP DNS lookup failed: {reason}")))?
                .collect::<Vec<_>>();
            if addresses.is_empty() {
                return Err(smtp_error(ErrorKind::DNS, "dns", &config.host, None, "SMTP DNS lookup returned no addresses"));
            }
            let mut last = None;
            let mut connected = None;
            for address in addresses {
                let budget = (runtime.remaining_ms)().map(|ms| ms.max(1) as u64).unwrap_or(30_000);
                let deadline = std::time::Instant::now() + std::time::Duration::from_millis(budget);
                loop {
                    control.checkpoint("connect").map_err(|stop| stop_error(stop, "connect", &config.host, false))?;
                    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                    if remaining.is_zero() { break; }
                    match std::net::TcpStream::connect_timeout(&address, remaining.min(std::time::Duration::from_millis(100))) {
                        Ok(stream) => { connected = Some(stream); break; }
                        Err(reason) if matches!(reason.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock) => last = Some(reason),
                        Err(reason) => { last = Some(reason); break; }
                    }
                }
                if connected.is_some() { break; }
            }
            let stream = connected.ok_or_else(|| smtp_error(
                ErrorKind::Connect, "connect", &config.host, None,
                format!("SMTP connection failed: {}", last.map(|e| e.to_string()).unwrap_or_else(|| "no address accepted the connection".to_string())),
            ))?;
            stream.set_nodelay(true).map_err(|reason| smtp_error(
                ErrorKind::Connect, "connect", &config.host, None, format!("SMTP socket setup failed: {reason}"),
            ))?;
            let poll = Some(std::time::Duration::from_millis(25));
            stream.set_read_timeout(poll).map_err(|reason| smtp_error(ErrorKind::Connect, "connect", &config.host, None, format!("SMTP read timeout setup failed: {reason}")))?;
            stream.set_write_timeout((runtime.remaining_ms)().map(|ms| std::time::Duration::from_millis(ms.max(1) as u64)))
                .map_err(|reason| smtp_error(ErrorKind::Connect, "connect", &config.host, None, format!("SMTP write timeout setup failed: {reason}")))?;
            let mut transport = RuntimeTransport {
                stream: RuntimeStream::Plain(Some(stream)), runtime, control, stopped: None,
            };
            if config.security == SMTPSecurity::TLS {
                transport.upgrade(&config.host, &config.trust).map_err(|reason| {
                    if let Some(stop) = transport.take_stop() { stop_error(stop, "connect_tls", &config.host, false) }
                    else { smtp_error(ErrorKind::TLS, "connect_tls", &config.host, None, reason) }
                })?;
            }
            Ok(transport)
        }

        fn poll_stop(&mut self) -> bool {
            match self.control.checkpoint("smtp_io") {
                Ok(()) => false,
                Err(stop) => { self.stopped = Some(stop); true }
            }
        }

        fn upgrade(&mut self, server: &str, trust: &TLSTrust) -> Result<(), String> {
            let RuntimeStream::Plain(slot) = &mut self.stream else {
                return Err("SMTP transport is not a plaintext stream".to_string());
            };
            let stream = slot.take().ok_or_else(|| "SMTP transport is closed".to_string())?;
            stream.set_read_timeout(None).map_err(|e| format!("TLS socket setup failed: {e}"))?;
            stream.set_write_timeout(None).map_err(|e| format!("TLS socket setup failed: {e}"))?;
            let id = match trust {
                TLSTrust::System => (self.runtime.tls_begin)(stream, &server.to_string()),
                TLSTrust::SystemPlusCa { pem } => (self.runtime.tls_begin_ca)(stream, &server.to_string(), pem),
            }?;
            self.stream = RuntimeStream::TLS(id);
            loop {
                if self.poll_stop() {
                    let _ = (self.runtime.tls_close)(id);
                    self.stream = RuntimeStream::Closed;
                    return Err("SMTP TLS handshake stopped".to_string());
                }
                if (self.runtime.tls_handshake_step)(id)? { break; }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            (self.runtime.tls_set_poll_timeout)(id, 25)?;
            Ok(())
        }
    }

    fn io_poll_timeout(reason: &str) -> bool {
        let lower = reason.to_ascii_lowercase();
        lower.contains("timed out") || lower.contains("would block") || lower.contains("temporarily unavailable")
    }

    impl std::io::Read for RuntimeTransport {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            loop {
                let result = match &mut self.stream {
                    RuntimeStream::Plain(Some(stream)) => std::io::Read::read(stream, out),
                    RuntimeStream::TLS(id) => (self.runtime.tls_read)(*id, out.len() as i64)
                        .map(|bytes| { let count = bytes.len(); out[..count].copy_from_slice(&bytes); count })
                        .map_err(std::io::Error::other),
                    _ => return Ok(0),
                };
                match result {
                    Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut)
                        || io_poll_timeout(&error.to_string()) => {
                        if self.poll_stop() { return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "SMTP operation stopped")); }
                    }
                    other => return other,
                }
            }
        }
    }

    impl std::io::Write for RuntimeTransport {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            match &mut self.stream {
                RuntimeStream::Plain(Some(stream)) => std::io::Write::write(stream, bytes),
                RuntimeStream::TLS(id) => {
                    let owned = bytes.to_vec();
                    (self.runtime.tls_write_all)(*id, &owned).map(|_| bytes.len()).map_err(std::io::Error::other)
                }
                _ => Err(std::io::Error::new(std::io::ErrorKind::NotConnected, "SMTP transport is closed")),
            }
        }

        fn flush(&mut self) -> std::io::Result<()> {
            match &mut self.stream {
                RuntimeStream::Plain(Some(stream)) => std::io::Write::flush(stream),
                RuntimeStream::TLS(_) => Ok(()),
                _ => Err(std::io::Error::new(std::io::ErrorKind::NotConnected, "SMTP transport is closed")),
            }
        }
    }

    impl SMTPTransport for RuntimeTransport {
        fn verified_tls(&self) -> bool { matches!(self.stream, RuntimeStream::TLS(_)) }
        fn start_tls(&mut self, server: &str, trust: &TLSTrust) -> Result<(), String> { self.upgrade(server, trust) }
        fn close(&mut self) {
            let old = std::mem::replace(&mut self.stream, RuntimeStream::Closed);
            if let RuntimeStream::TLS(id) = old { let _ = (self.runtime.tls_close)(id); }
        }
        fn take_stop(&mut self) -> Option<SMTPStop> { self.stopped.take() }
    }

    fn relaxed_body(body: &[u8]) -> Result<Vec<u8>, Error> {
        let text = std::str::from_utf8(body)
            .map_err(|_| error("dkim", "serialized message body must be UTF-8"))?;
        let mut lines: Vec<&str> = text.split("\r\n").collect();
        while lines.last().is_some_and(|line| line.trim_end_matches([' ', '\t']).is_empty()) {
            lines.pop();
        }
        let mut out = Vec::new();
        for line in lines {
            out.extend_from_slice(line.trim_end_matches([' ', '\t']).as_bytes());
            out.extend_from_slice(b"\r\n");
        }
        if out.is_empty() { out.extend_from_slice(b"\r\n"); }
        Ok(out)
    }

    fn relaxed_header(header: &[u8]) -> Result<Vec<u8>, Error> {
        let colon = header.iter().position(|byte| *byte == b':')
            .ok_or_else(|| error("dkim", "serialized message contains a malformed header"))?;
        let name = std::str::from_utf8(&header[..colon])
            .map_err(|_| error("dkim", "serialized header name must be ASCII"))?
            .to_ascii_lowercase();
        let mut value = Vec::new();
        let mut index = colon + 1;
        let mut whitespace = false;
        while index < header.len() {
            if index + 2 < header.len() && &header[index..index + 2] == b"\r\n"
                && matches!(header[index + 2], b' ' | b'\t')
            {
                whitespace = true;
                index += 3;
                continue;
            }
            let byte = header[index];
            if matches!(byte, b' ' | b'\t') {
                whitespace = true;
            } else {
                if whitespace && !value.is_empty() { value.push(b' '); }
                whitespace = false;
                value.push(byte);
            }
            index += 1;
        }
        while value.last() == Some(&b' ') { value.pop(); }
        let mut out = name.into_bytes();
        out.push(b':');
        out.extend_from_slice(&value);
        out.extend_from_slice(b"\r\n");
        Ok(out)
    }

    fn message_headers(wire: &[u8]) -> Result<(Vec<Vec<u8>>, &[u8]), Error> {
        let split = wire.windows(4).position(|window| window == b"\r\n\r\n")
            .ok_or_else(|| error("dkim", "serialized message is missing its header/body boundary"))?;
        let raw = &wire[..split];
        let mut headers: Vec<Vec<u8>> = Vec::new();
        for line in raw.split(|byte| *byte == b'\n') {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            if matches!(line.first(), Some(b' ' | b'\t')) {
                let Some(previous) = headers.last_mut() else {
                    return Err(error("dkim", "serialized message starts with a folded header"));
                };
                previous.extend_from_slice(b"\r\n");
                previous.extend_from_slice(line);
            } else {
                headers.push(line.to_vec());
            }
        }
        Ok((headers, &wire[split + 4..]))
    }

    fn header_name(header: &[u8]) -> Option<String> {
        let colon = header.iter().position(|byte| *byte == b':')?;
        std::str::from_utf8(&header[..colon]).ok().map(str::to_ascii_lowercase)
    }

    fn dkim_sign(wire: Vec<u8>, dkim: &DkimConfig<Vec<u8>>, runtime: RuntimeFns)
        -> Result<Vec<u8>, Error>
    {
        let (headers, body) = message_headers(&wire)?;
        let names: Vec<String> = dkim.signed_headers.iter().map(|name| name.to_ascii_lowercase()).collect();
        let mut signed = Vec::new();
        for name in &names {
            let header = headers.iter().rev().find(|header| header_name(header).as_deref() == Some(name))
                .ok_or_else(|| error("dkim", format!("signed header `{name}` is absent from the final message")))?;
            signed.extend_from_slice(&relaxed_header(header)?);
        }
        let body_hash = base64(&(runtime.sha256)(&relaxed_body(body)?));
        let header_list = names.join(":");
        let unsigned_value = format!(
            "v=1; a=ed25519-sha256; c=relaxed/relaxed; d={}; s={}; h={}; bh={}; b=",
            dkim.domain, dkim.selector, header_list, body_hash,
        );
        signed.extend_from_slice(&relaxed_header(format!("DKIM-Signature: {unsigned_value}").as_bytes())?);
        let header_hash = (runtime.sha256)(&signed);
        let signature = (runtime.ed25519_sign)(&dkim.private_key, &header_hash)
            .map_err(|_| error("dkim", "Ed25519 signing failed"))?;
        if signature.len() != 64 {
            return Err(error("dkim", "Ed25519 signer returned an invalid signature"));
        }
        let signature = base64(&signature);
        let folded = format!(
            "DKIM-Signature: v=1; a=ed25519-sha256; c=relaxed/relaxed;\r\n d={}; s={};\r\n h={};\r\n bh={};\r\n b={}\r\n",
            dkim.domain, dkim.selector, header_list, body_hash, signature,
        );
        let mut out = Vec::with_capacity(folded.len() + wire.len());
        out.extend_from_slice(folded.as_bytes());
        out.extend_from_slice(&wire);
        Ok(out)
    }

    fn prepare_message(config: &SMTPConfig<Vec<u8>>, message: &Message, runtime: Option<RuntimeFns>)
        -> Result<Vec<u8>, Error>
    {
        validate_smtp_config(config)?;
        if config.dkim.as_ref().is_ok_and(|dkim| dkim.private_key.len() != 32) {
            return Err(error("dkim", "private_key must contain exactly 32 bytes"));
        }
        let mut wire = serialize(message)?;
        if let Ok(dkim) = &config.dkim {
            let runtime = runtime.ok_or_else(|| error("dkim", "signing runtime is unavailable"))?;
            wire = dkim_sign(wire, dkim, runtime)?;
        }
        if wire.len() > config.limits.max_message_bytes as usize {
            return Err(smtp_error(ErrorKind::Configuration, "message", &config.host, None,
                "serialized message including DKIM exceeds configured max_message_bytes"));
        }
        Ok(wire)
    }

    impl Mailer {
        pub fn send(&mut self, message: Message) -> Result<SendReport, Error> {
            let control = AmbientSmtpControl(self.runtime);
            let mime = prepare_message(&self.config, &message, Some(self.runtime))?;
            let mut transport = RuntimeTransport::connect(&self.config, self.runtime)?;
            let result = smtp_transaction_inner(&mut transport, &self.config, &message, &mime, &control);
            transport.close();
            result
        }
    }

    pub fn smtp_transaction<T: SMTPTransport, C: SMTPControl>(
        transport: &mut T,
        config: &SMTPConfig<Vec<u8>>,
        message: &Message,
        control: &C,
    ) -> Result<SendReport, Error> {
        let mime = prepare_message(config, message, None)?;
        let result = smtp_transaction_inner(transport, config, message, &mime, control);
        transport.close();
        result
    }

    fn smtp_transaction_inner<T: SMTPTransport, C: SMTPControl>(
        transport: &mut T,
        config: &SMTPConfig<Vec<u8>>,
        message: &Message,
        mime: &[u8],
        control: &C,
    ) -> Result<SendReport, Error> {
        validate_smtp_config(config)?;
        if message.envelope.recipients.len() > config.limits.max_recipients as usize {
            return Err(smtp_error(
                ErrorKind::Configuration, "recipients", &config.host, None,
                "envelope exceeds configured max_recipients",
            ));
        }
        if let SMTPAuth::Password { password, .. } = &config.auth {
            std::str::from_utf8(password).map_err(|_| smtp_error(
                ErrorKind::Configuration, "auth", &config.host, None,
                "SMTP password must be UTF-8",
            ))?;
        }

        checkpoint(control, "greeting", &config.host, false)?;
        if config.security == SMTPSecurity::TLS && !transport.verified_tls() {
            return Err(smtp_error(
                ErrorKind::TLS, "connect_tls", &config.host, None,
                "implicit TLS transport is not verified",
            ));
        }
        let greeting = read_reply(transport, config, "greeting", false)?;
        let mut state = SMTPState::new();
        state.greeting(&greeting).map_err(|error| with_server(error, &config.host))?;
        if config.security == SMTPSecurity::TLS { state.verified_tls(); }

        let mut capabilities = ehlo(transport, config, control, &mut state)?;
        if config.security == SMTPSecurity::StartTls {
            state.start_tls(&capabilities).map_err(|error| with_server(error, &config.host))?;
            command(transport, control, config, "starttls", b"STARTTLS\r\n", false)?;
            let reply = read_reply(transport, config, "starttls", false)?;
            expect_code(&reply, 220, "starttls", &config.host)?;
            checkpoint(control, "tls_handshake", &config.host, false)?;
            if let Err(reason) = transport.start_tls(&config.host, &config.trust) {
                return Err(transport_failure(transport, "starttls", &config.host, false, ErrorKind::TLS, reason));
            }
            if !transport.verified_tls() {
                return Err(smtp_error(
                    ErrorKind::TLS, "starttls", &config.host, None,
                    "STARTTLS completed without verified peer identity",
                ));
            }
            state.verified_tls();
            capabilities = ehlo(transport, config, control, &mut state)?;
        }

        authenticate(transport, config, control, &mut state, &capabilities)?;

        let mail = format!("MAIL FROM:<{}>\r\n", message.envelope.from.mailbox);
        command(transport, control, config, "mail_from", mail.as_bytes(), false)?;
        let mail_reply = read_reply(transport, config, "mail_from", false)?;
        expect_success(&mail_reply, "mail_from", &config.host)?;

        let mut accepted = Vec::new();
        let mut rejected = Vec::new();
        for recipient in &message.envelope.recipients {
            let rcpt = format!("RCPT TO:<{}>\r\n", recipient.mailbox);
            command(transport, control, config, "rcpt_to", rcpt.as_bytes(), false)?;
            let reply = read_reply(transport, config, "rcpt_to", false)?;
            let report = RecipientReport {
                address: recipient.clone(),
                accepted: matches!(reply.code, 250 | 251 | 252),
                code: reply.code,
                message: reply_text(&reply),
            };
            if report.accepted {
                accepted.push(report);
            } else if (400..=599).contains(&reply.code) {
                rejected.push(report);
            } else {
                return Err(smtp_error(
                    ErrorKind::Protocol, "rcpt_to", &config.host, Some(reply.code),
                    "RCPT TO returned an unexpected response code",
                ));
            }
        }

        if accepted.is_empty() || (config.recipient_policy == RecipientPolicy::RequireAll && !rejected.is_empty()) {
            let refusal = rejected.first().expect("recipient outcome must include a refusal");
            return Err(reply_failure("rcpt_to", &config.host, refusal.code, &refusal.message));
        }

        command(transport, control, config, "data", b"DATA\r\n", false)?;
        let data_reply = read_reply(transport, config, "data", false)?;
        expect_code(&data_reply, 354, "data", &config.host)?;
        checkpoint(control, "data_body", &config.host, false)?;
        let wire = dot_stuff(mime, config.limits.max_message_bytes as usize)?;
        if let Err(reason) = transport.write_all(&wire) {
            return Err(transport_failure(transport, "data_body", &config.host, true, ErrorKind::DeliveryUnknown,
                format!("connection failed after DATA began: {reason}")));
        }
        if let Err(reason) = transport.flush() {
            return Err(transport_failure(transport, "data_body", &config.host, true, ErrorKind::DeliveryUnknown,
                format!("connection failed while flushing DATA: {reason}")));
        }
        checkpoint(control, "data_response", &config.host, true)?;
        let final_reply = read_reply(transport, config, "data_response", true)?;
        expect_success(&final_reply, "data_response", &config.host)?;

        let report = SendReport {
            server: config.host.clone(),
            accepted,
            rejected,
            response_code: final_reply.code,
            response: reply_text(&final_reply),
            accepted_at: control.accepted_at(),
        };
        let _ = command(transport, control, config, "quit", b"QUIT\r\n", false)
            .and_then(|_| read_reply(transport, config, "quit", false).map(|_| ()));
        Ok(report)
    }

    fn ehlo<T: SMTPTransport, C: SMTPControl>(
        transport: &mut T,
        config: &SMTPConfig<Vec<u8>>,
        control: &C,
        state: &mut SMTPState,
    ) -> Result<SMTPCapabilities, Error> {
        command(transport, control, config, "ehlo", b"EHLO localhost\r\n", false)?;
        let reply = read_reply(transport, config, "ehlo", false)?;
        state.ehlo(&reply, &config.limits).map_err(|error| with_server(error, &config.host))
    }

    fn authenticate<T: SMTPTransport, C: SMTPControl>(
        transport: &mut T,
        config: &SMTPConfig<Vec<u8>>,
        control: &C,
        state: &mut SMTPState,
        capabilities: &SMTPCapabilities,
    ) -> Result<(), Error> {
        let SMTPAuth::Password { username, password } = &config.auth else { return Ok(()); };
        let mechanism = if capabilities.auth_mechanisms.iter().any(|item| item == "PLAIN") {
            "PLAIN"
        } else if capabilities.auth_mechanisms.iter().any(|item| item == "LOGIN") {
            "LOGIN"
        } else {
            return Err(smtp_error(
                ErrorKind::Auth, "auth", &config.host, None,
                "relay does not offer PLAIN or LOGIN authentication",
            ));
        };
        state.authenticate(mechanism, 0, &config.limits)
            .map_err(|error| with_server(error, &config.host))?;
        if mechanism == "PLAIN" {
            let mut payload = Vec::with_capacity(username.len().saturating_add(password.len()).saturating_add(2));
            payload.push(0);
            payload.extend_from_slice(username.as_bytes());
            payload.push(0);
            payload.extend_from_slice(password);
            let mut line = b"AUTH PLAIN ".to_vec();
            let mut encoded = base64(&payload).into_bytes();
            line.extend_from_slice(&encoded);
            line.extend_from_slice(b"\r\n");
            let sent = command(transport, control, config, "auth", &line, false);
            control.wipe(&mut payload);
            control.wipe(&mut encoded);
            control.wipe(&mut line);
            sent?;
            let reply = read_reply(transport, config, "auth", false)?;
            expect_auth_success(&reply, config)
        } else {
            command(transport, control, config, "auth", b"AUTH LOGIN\r\n", false)?;
            let username_challenge = read_reply(transport, config, "auth_username", false)?;
            expect_auth_challenge(&username_challenge, config)?;
            let line = format!("{}\r\n", base64(username.as_bytes()));
            command(transport, control, config, "auth_username", line.as_bytes(), false)?;
            let password_challenge = read_reply(transport, config, "auth_password", false)?;
            expect_auth_challenge(&password_challenge, config)?;
            let mut line = base64(password).into_bytes();
            line.extend_from_slice(b"\r\n");
            let sent = command(transport, control, config, "auth_password", &line, false);
            control.wipe(&mut line);
            sent?;
            let reply = read_reply(transport, config, "auth", false)?;
            expect_auth_success(&reply, config)
        }
    }

    fn expect_auth_success(reply: &SMTPReply, config: &SMTPConfig<Vec<u8>>) -> Result<(), Error> {
        if reply.code == 235 { return Ok(()); }
        if (400..=599).contains(&reply.code) {
            return Err(smtp_error(
                ErrorKind::Auth, "auth", &config.host, Some(reply.code), reply_text(reply),
            ));
        }
        Err(smtp_error(
            ErrorKind::Protocol, "auth", &config.host, Some(reply.code),
            format!("expected SMTP 235, got {}", reply.code),
        ))
    }

    fn expect_auth_challenge(reply: &SMTPReply, config: &SMTPConfig<Vec<u8>>) -> Result<(), Error> {
        expect_code(reply, 334, "auth", &config.host)?;
        if reply.lines.iter().map(String::len).sum::<usize>() > config.limits.max_auth_challenge_bytes as usize {
            return Err(smtp_error(
                ErrorKind::Auth, "auth", &config.host, Some(reply.code),
                "authentication challenge exceeds max_auth_challenge_bytes",
            ));
        }
        Ok(())
    }

    fn command<T: SMTPTransport, C: SMTPControl>(
        transport: &mut T,
        control: &C,
        config: &SMTPConfig<Vec<u8>>,
        operation: &str,
        bytes: &[u8],
        ambiguous: bool,
    ) -> Result<(), Error> {
        checkpoint(control, operation, &config.host, ambiguous)?;
        if let Err(reason) = transport.write_all(bytes) {
            return Err(transport_failure(transport, operation, &config.host, ambiguous,
                if ambiguous { ErrorKind::DeliveryUnknown } else { ErrorKind::Connect },
                format!("SMTP write failed: {reason}")));
        }
        if let Err(reason) = transport.flush() {
            return Err(transport_failure(transport, operation, &config.host, ambiguous,
                if ambiguous { ErrorKind::DeliveryUnknown } else { ErrorKind::Connect },
                format!("SMTP flush failed: {reason}")));
        }
        Ok(())
    }

    fn read_reply<T: SMTPTransport>(
        transport: &mut T,
        config: &SMTPConfig<Vec<u8>>,
        operation: &str,
        ambiguous: bool,
    ) -> Result<SMTPReply, Error> {
        read_smtp_reply(transport, &config.limits).map_err(|error| {
            if let Some(stop) = transport.take_stop() {
                return stop_error(stop, operation, &config.host, ambiguous);
            }
            if ambiguous {
                smtp_error(
                    ErrorKind::DeliveryUnknown, operation, &config.host, None,
                    format!("relay acceptance is unknown: {}", error_reason(&error)),
                )
            } else {
                with_operation_server(error, operation, &config.host)
            }
        })
    }

    pub fn smtp_dot_stuff(bytes: &[u8]) -> Result<Vec<u8>, Error> { dot_stuff(bytes, usize::MAX) }

    fn dot_stuff(bytes: &[u8], max_message_bytes: usize) -> Result<Vec<u8>, Error> {
        let leading_dots = bytes.windows(3).filter(|window| *window == b"\r\n.").count()
            + usize::from(bytes.first() == Some(&b'.'));
        let final_crlf = if bytes.ends_with(b"\r\n") { 0 } else { 2 };
        let capacity = bytes.len().checked_add(leading_dots)
            .and_then(|value| value.checked_add(final_crlf))
            .and_then(|value| value.checked_add(3))
            .ok_or_else(|| error("smtp", "dot-stuffed message size overflow"))?;
        if capacity.saturating_sub(3) > max_message_bytes {
            return Err(error("smtp", "dot-stuffed message exceeds configured max_message_bytes"));
        }
        let mut out = Vec::with_capacity(capacity);
        let mut line_start = true;
        for byte in bytes {
            if line_start && *byte == b'.' { out.push(b'.'); }
            out.push(*byte);
            line_start = out.ends_with(b"\r\n");
        }
        if final_crlf != 0 { out.extend_from_slice(b"\r\n"); }
        out.extend_from_slice(b".\r\n");
        Ok(out)
    }

    fn checkpoint<C: SMTPControl>(
        control: &C,
        operation: &str,
        server: &str,
        ambiguous: bool,
    ) -> Result<(), Error> {
        control.checkpoint(operation).map_err(|stop| stop_error(stop, operation, server, ambiguous))
    }

    fn stop_error(stop: SMTPStop, operation: &str, server: &str, ambiguous: bool) -> Error {
        if ambiguous {
            smtp_error(ErrorKind::DeliveryUnknown, operation, server, None,
                "operation stopped after DATA was transmitted; relay acceptance is unknown")
        } else {
            smtp_error(
                match stop { SMTPStop::Cancelled => ErrorKind::Cancelled, SMTPStop::TimedOut => ErrorKind::TimedOut },
                operation, server, None,
                match stop { SMTPStop::Cancelled => "SMTP operation cancelled", SMTPStop::TimedOut => "SMTP operation timed out" },
            )
        }
    }

    fn transport_failure<T: SMTPTransport>(
        transport: &mut T,
        operation: &str,
        server: &str,
        ambiguous: bool,
        fallback: ErrorKind,
        reason: impl Into<String>,
    ) -> Error {
        if let Some(stop) = transport.take_stop() { stop_error(stop, operation, server, ambiguous) }
        else { smtp_error(fallback, operation, server, None, reason) }
    }

    fn expect_success(reply: &SMTPReply, operation: &str, server: &str) -> Result<(), Error> {
        if (200..=299).contains(&reply.code) { Ok(()) }
        else if (400..=599).contains(&reply.code) {
            Err(reply_failure(operation, server, reply.code, &reply_text(reply)))
        } else {
            Err(smtp_error(
                ErrorKind::Protocol, operation, server, Some(reply.code),
                "SMTP command returned an unexpected response code",
            ))
        }
    }

    fn expect_code(reply: &SMTPReply, code: i64, operation: &str, server: &str) -> Result<(), Error> {
        if reply.code == code { Ok(()) }
        else if (400..=599).contains(&reply.code) {
            Err(reply_failure(operation, server, reply.code, &reply_text(reply)))
        } else {
            Err(smtp_error(
                ErrorKind::Protocol, operation, server, Some(reply.code),
                format!("expected SMTP {code}, got {}", reply.code),
            ))
        }
    }

    fn reply_failure(operation: &str, server: &str, code: i64, reason: &str) -> Error {
        smtp_error(
            if code < 500 { ErrorKind::Transient } else { ErrorKind::Rejected },
            operation, server, Some(code), reason,
        )
    }

    fn reply_text(reply: &SMTPReply) -> String { reply.lines.join("\n") }

    fn error_reason(error: &Error) -> &str {
        match error {
            Error::Configuration { reason, .. } | Error::DNS { reason, .. }
            | Error::Connect { reason, .. } | Error::TLS { reason, .. }
            | Error::Auth { reason, .. } | Error::Protocol { reason, .. }
            | Error::Rejected { reason, .. } | Error::Transient { reason, .. }
            | Error::TimedOut { reason, .. } | Error::Cancelled { reason, .. }
            | Error::DeliveryUnknown { reason, .. } => reason,
        }
    }

    fn with_server(error: Error, server: &str) -> Error {
        with_operation_server(error, "smtp", server)
    }

    fn with_operation_server(error: Error, operation: &str, server: &str) -> Error {
        let reason = error_reason(&error).to_string();
        let code = match &error {
            Error::Configuration { code, .. } | Error::DNS { code, .. }
            | Error::Connect { code, .. } | Error::TLS { code, .. }
            | Error::Auth { code, .. } | Error::Protocol { code, .. }
            | Error::Rejected { code, .. } | Error::Transient { code, .. }
            | Error::TimedOut { code, .. } | Error::Cancelled { code, .. }
            | Error::DeliveryUnknown { code, .. } => *code,
        };
        let kind = match error {
            Error::Configuration { .. } => ErrorKind::Configuration,
            Error::DNS { .. } => ErrorKind::DNS,
            Error::Connect { .. } => ErrorKind::Connect,
            Error::TLS { .. } => ErrorKind::TLS,
            Error::Auth { .. } => ErrorKind::Auth,
            Error::Protocol { .. } => ErrorKind::Protocol,
            Error::Rejected { .. } => ErrorKind::Rejected,
            Error::Transient { .. } => ErrorKind::Transient,
            Error::TimedOut { .. } => ErrorKind::TimedOut,
            Error::Cancelled { .. } => ErrorKind::Cancelled,
            Error::DeliveryUnknown { .. } => ErrorKind::DeliveryUnknown,
        };
        smtp_error(kind, operation, server, code, reason)
    }

    #[derive(Clone, Copy)]
    enum ErrorKind {
        Configuration, DNS, Connect, TLS, Auth, Protocol, Rejected, Transient,
        TimedOut, Cancelled, DeliveryUnknown,
    }

    fn smtp_error(
        kind: ErrorKind,
        operation: &str,
        server: &str,
        code: Option<i64>,
        reason: impl Into<String>,
    ) -> Error {
        let operation = operation.to_string();
        let server = Some(server.to_string());
        let reason = reason.into();
        match kind {
            ErrorKind::Configuration => Error::Configuration { operation, server, code, reason },
            ErrorKind::DNS => Error::DNS { operation, server, code, reason },
            ErrorKind::Connect => Error::Connect { operation, server, code, reason },
            ErrorKind::TLS => Error::TLS { operation, server, code, reason },
            ErrorKind::Auth => Error::Auth { operation, server, code, reason },
            ErrorKind::Protocol => Error::Protocol { operation, server, code, reason },
            ErrorKind::Rejected => Error::Rejected { operation, server, code, reason },
            ErrorKind::Transient => Error::Transient { operation, server, code, reason },
            ErrorKind::TimedOut => Error::TimedOut { operation, server, code, reason },
            ErrorKind::Cancelled => Error::Cancelled { operation, server, code, reason },
            ErrorKind::DeliveryUnknown => Error::DeliveryUnknown { operation, server, code, reason },
        }
    }

    fn error(operation: &'static str, reason: impl Into<String>) -> Error {
        Error::Configuration {
            operation: operation.to_string(), server: None, code: None, reason: reason.into(),
        }
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
        let envelope = default_envelope(from, to, bcc)?;
        Ok(Message {
            from: from.clone(), to: to.clone(), bcc: bcc.clone(), subject: subject.clone(),
            text: text.clone(), html: html.clone(), attachments: attachments.clone(), envelope, wire_upper,
        })
    }

    fn default_envelope(from: &Address, to: &[Address], bcc: &[Address]) -> Result<Envelope, Error> {
        let mut recipients = Vec::with_capacity(to.len().saturating_add(bcc.len()));
        recipients.extend_from_slice(to);
        recipients.extend_from_slice(bcc);
        envelope(from, &recipients)
    }

    pub fn envelope(from: &Address, recipients: &Vec<Address>) -> Result<Envelope, Error> {
        if recipients.is_empty() {
            return Err(error("envelope", "email envelope needs at least one recipient"));
        }
        if recipients.len() > MAX_RECIPIENTS {
            return Err(error("envelope", format!("email envelope exceeds {MAX_RECIPIENTS} recipients")));
        }
        Ok(Envelope { from: from.clone(), recipients: recipients.clone() })
    }

    impl Message {
        pub fn envelope(&self) -> &Envelope { &self.envelope }

        pub fn with_envelope(&self, envelope: &Envelope) -> Result<Message, Error> {
            if envelope.recipients.is_empty() || envelope.recipients.len() > MAX_RECIPIENTS {
                return Err(error("with_envelope", "email envelope recipient count is outside the supported range"));
            }
            let mut message = self.clone();
            message.envelope = envelope.clone();
            Ok(message)
        }
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
