// D-HTTP-CLIENT2=A / D-DEP-HTTP2=B: native core.http client transport.
//
// This source is emitted into the generated program's hidden bridge crate. HTTP
// parsing, pooling, redirects, proxies, retries, cookies, and compression are
// std-only. The separately-ratified rustls/system-root bridge remains the sole
// external transport seam for HTTPS.

use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

thread_local! {
    /// Absolute wall Instant for the ambient `@Context(deadline:)` budget, or None.
    static HTTP_AMBIENT_DEADLINE: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// RAII ambient `@Context(deadline:)` upper bound for one HTTP send (D-HTTP-CLIENT2).
///
/// Converts remaining milliseconds into an absolute Instant at push time so later
/// request prep cannot revive a stale residual budget with `Instant::now() + ms`.
pub struct JetHttpAmbientDeadline {
    previous: Option<Instant>,
}

impl JetHttpAmbientDeadline {
    pub fn push(remaining_ms: Option<i64>) -> Result<Self, JetHttpBridgeError> {
        let absolute = match remaining_ms {
            Some(ms) if ms <= 0 => return Err(JetHttpBridgeError::Timeout),
            Some(ms) => Some(Instant::now() + validated_timeout("ambient deadline", ms)?),
            None => None,
        };
        let previous = HTTP_AMBIENT_DEADLINE.with(|cell| cell.replace(absolute));
        Ok(Self { previous })
    }
}

impl Drop for JetHttpAmbientDeadline {
    fn drop(&mut self) {
        HTTP_AMBIENT_DEADLINE.with(|cell| cell.set(self.previous));
    }
}

fn ambient_deadline_instant() -> Option<Instant> {
    HTTP_AMBIENT_DEADLINE.with(|cell| cell.get())
}

fn compose_total_deadline(
    configured: Option<Duration>,
) -> Result<Option<Instant>, JetHttpBridgeError> {
    let ambient = match ambient_deadline_instant() {
        Some(deadline) if deadline <= Instant::now() => return Err(JetHttpBridgeError::Timeout),
        other => other,
    };
    Ok(match (configured, ambient) {
        (Some(configured), Some(ambient)) => Some((Instant::now() + configured).min(ambient)),
        (Some(configured), None) => Some(Instant::now() + configured),
        (None, Some(ambient)) => Some(ambient),
        (None, None) => None,
    })
}

const HTTP_CLIENT_DEFAULT_REDIRECTS: u32 = 10;
const HTTP_UPLOAD_CHUNK: usize = 64 * 1024;
/// Cap for buffering a streaming upload so 307/308 can replay it. Matches the
/// former `Body.bytes(1GiB)` send ceiling; oversize fails closed.
const HTTP_UPLOAD_REPLAY_CAP: usize = 1024 * 1024 * 1024;

/// Private, typed transport failures. Generated code exhaustively projects these
/// to the public closed HttpError without carrying backend prose across the seam.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetHttpBridgeError {
    InvalidUrl,
    InvalidHeader,
    InvalidFraming,
    UnsupportedEncoding,
    Resolve,
    Connect,
    Tls,
    Timeout,
    Proxy,
    Redirect,
    Protocol,
    Io,
    ResourceUnavailable,
    Cancelled,
    Internal,
}

type BodyReader = Arc<Mutex<Box<dyn Read + Send>>>;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct PoolKey {
    namespace: i64,
    scheme: String,
    host: String,
    port: u16,
    proxy: Option<String>,
    protocol: &'static str,
}

struct IdleConnection {
    stream: HttpStream,
    idle_since: Instant,
}

#[derive(Default)]
struct ClientPool {
    idle: HashMap<PoolKey, VecDeque<IdleConnection>>,
    idle_total: usize,
}

impl ClientPool {
    fn take(&mut self, key: &PoolKey) -> Option<HttpStream> {
        self.expire();
        let entries = self.idle.get_mut(key)?;
        if let Some(IdleConnection {
            stream: HttpStream::H2(connection),
            idle_since,
        }) = entries.back_mut()
        {
            *idle_since = Instant::now();
            return Some(HttpStream::H2(connection.clone()));
        }
        let stream = entries.pop_back()?.stream;
        self.idle_total = self.idle_total.saturating_sub(1);
        Some(stream)
    }

    fn put(&mut self, key: PoolKey, stream: HttpStream) {
        self.expire();
        let origin = self.idle.entry(key).or_default();
        if matches!(&stream, HttpStream::H2(_))
            && origin
                .iter()
                .any(|entry| matches!(&entry.stream, HttpStream::H2(_)))
        {
            return;
        }
        if origin.len() == 8 || self.idle_total == 64 {
            return;
        }
        origin.push_back(IdleConnection {
            stream,
            idle_since: Instant::now(),
        });
        self.idle_total += 1;
    }

    fn remove_h2(&mut self, key: &PoolKey, stale: &Arc<Mutex<H2Connection>>) {
        let Some(entries) = self.idle.get_mut(key) else {
            return;
        };
        let before = entries.len();
        entries.retain(|entry| {
            !matches!(
                &entry.stream,
                HttpStream::H2(connection) if Arc::ptr_eq(connection, stale)
            )
        });
        self.idle_total = self
            .idle_total
            .saturating_sub(before.saturating_sub(entries.len()));
    }

    fn expire(&mut self) {
        let cutoff = Instant::now() - Duration::from_secs(90);
        for entries in self.idle.values_mut() {
            while entries.front().is_some_and(|entry| {
                entry.idle_since <= cutoff
                    && match &entry.stream {
                        HttpStream::H2(connection) => Arc::strong_count(connection) == 1,
                        _ => true,
                    }
            }) {
                entries.pop_front();
                self.idle_total = self.idle_total.saturating_sub(1);
            }
        }
    }
}

fn default_client_pool() -> &'static Arc<Mutex<ClientPool>> {
    static POOL: OnceLock<Arc<Mutex<ClientPool>>> = OnceLock::new();
    POOL.get_or_init(|| Arc::new(Mutex::new(ClientPool::default())))
}

#[derive(Default)]
struct OriginLimits {
    counts: Mutex<HashMap<PoolKey, usize>>,
    ready: Condvar,
}

impl OriginLimits {
    fn acquire(
        self: &Arc<Self>,
        key: PoolKey,
        deadline: Option<Instant>,
    ) -> Result<OriginPermit, JetHttpBridgeError> {
        let mut counts = self
            .counts
            .lock()
            .map_err(|_| JetHttpBridgeError::Internal)?;
        loop {
            let count = counts.entry(key.clone()).or_default();
            if *count < 64 {
                *count += 1;
                return Ok(OriginPermit {
                    limits: self.clone(),
                    key: Some(key),
                });
            }
            counts = match deadline {
                Some(deadline) => {
                    let remaining = deadline
                        .checked_duration_since(Instant::now())
                        .ok_or(JetHttpBridgeError::Timeout)?;
                    let (counts, waited) = self
                        .ready
                        .wait_timeout(counts, remaining)
                        .map_err(|_| JetHttpBridgeError::Internal)?;
                    if waited.timed_out() {
                        return Err(JetHttpBridgeError::Timeout);
                    }
                    counts
                }
                None => self
                    .ready
                    .wait(counts)
                    .map_err(|_| JetHttpBridgeError::Internal)?,
            };
        }
    }

    fn release(&self, key: &PoolKey) {
        if let Ok(mut counts) = self.counts.lock() {
            if let Some(count) = counts.get_mut(key) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    counts.remove(key);
                }
            }
            self.ready.notify_one();
        }
    }
}

struct OriginPermit {
    limits: Arc<OriginLimits>,
    key: Option<PoolKey>,
}

impl Drop for OriginPermit {
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            self.limits.release(&key);
        }
    }
}

fn default_origin_limits() -> &'static Arc<OriginLimits> {
    static LIMITS: OnceLock<Arc<OriginLimits>> = OnceLock::new();
    LIMITS.get_or_init(|| Arc::new(OriginLimits::default()))
}

/// D-HTTP-CLIENT2: automatic retries only for a stale pooled connection *before
/// request bytes* (max one), never keyed off response status or post-write I/O.
/// Default `Safe` covers GET/HEAD/OPTIONS/TRACE; `.retries(.Idempotent)` opts in
/// PUT/DELETE; `None` disables. POST/PATCH never auto-retry.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RetryPolicy {
    None,
    Safe,
    Idempotent,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::Safe
    }
}

#[derive(Clone)]
struct ClientPolicy {
    redirect_limit: u32,
    /// When true (default), same-origin redirects keep Authorization /
    /// Proxy-Authorization / Cookie. Cross-origin redirects always strip them
    /// (D-HTTP-CLIENT2). When false, strip credentials on every redirect hop.
    same_origin_credentials: bool,
    allow_http_downgrade: bool,
    retry_policy: RetryPolicy,
    proxy: Option<String>,
    use_environment_proxy: bool,
    cookies: bool,
    dns_timeout: Duration,
    connect_timeout: Duration,
    tls_timeout: Duration,
    write_timeout: Duration,
    first_byte_timeout: Duration,
    read_timeout: Duration,
    total_timeout: Option<Duration>,
    http2: bool,
    http11: bool,
    h2c: bool,
    decompress: bool,
    system_roots: bool,
    custom_roots: Vec<Vec<u8>>,
    tls_min_version: i64,
    tls_max_version: i64,
    tls_identity_cert: Vec<u8>,
    tls_identity_key: Vec<u8>,
}

impl Default for ClientPolicy {
    fn default() -> Self {
        Self {
            redirect_limit: HTTP_CLIENT_DEFAULT_REDIRECTS,
            same_origin_credentials: true,
            allow_http_downgrade: false,
            retry_policy: RetryPolicy::Safe,
            proxy: None,
            use_environment_proxy: true,
            cookies: false,
            dns_timeout: Duration::from_secs(10),
            connect_timeout: Duration::from_secs(10),
            tls_timeout: Duration::from_secs(10),
            write_timeout: Duration::from_secs(30),
            first_byte_timeout: Duration::from_secs(30),
            read_timeout: Duration::from_secs(30),
            total_timeout: Some(Duration::from_secs(30)),
            http2: true,
            http11: true,
            h2c: false,
            decompress: true,
            system_roots: true,
            custom_roots: Vec::new(),
            tls_min_version: 12,
            tls_max_version: 13,
            tls_identity_cert: Vec::new(),
            tls_identity_key: Vec::new(),
        }
    }
}

struct ClientShared {
    pool: Arc<Mutex<ClientPool>>,
    jar: Arc<Mutex<CookieJar>>,
    dns: Arc<Mutex<DnsCache>>,
    limits: Arc<OriginLimits>,
}

#[derive(Clone)]
struct ClientHandle {
    namespace: i64,
    shared: Arc<ClientShared>,
    policy: ClientPolicy,
}

#[derive(Clone, Copy)]
struct TlsSettings<'a> {
    system_roots: bool,
    custom_roots: &'a [Vec<u8>],
    min_version: i64,
    max_version: i64,
    identity_cert: &'a [u8],
    identity_key: &'a [u8],
}

impl TlsSettings<'static> {
    const SYSTEM: Self = Self {
        system_roots: true,
        custom_roots: &[],
        min_version: 12,
        max_version: 13,
        identity_cert: &[],
        identity_key: &[],
    };
}

impl ClientPolicy {
    fn tls_settings(&self) -> TlsSettings<'_> {
        TlsSettings {
            system_roots: self.system_roots,
            custom_roots: &self.custom_roots,
            min_version: self.tls_min_version,
            max_version: self.tls_max_version,
            identity_cert: &self.tls_identity_cert,
            identity_key: &self.tls_identity_key,
        }
    }
}

fn client_handles() -> &'static Mutex<HashMap<i64, ClientHandle>> {
    static HANDLES: OnceLock<Mutex<HashMap<i64, ClientHandle>>> = OnceLock::new();
    HANDLES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_client_handle() -> i64 {
    static NEXT: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

pub fn jet_http_client_new_impl() -> i64 {
    let id = next_client_handle();
    let handle = ClientHandle {
        namespace: id,
        shared: Arc::new(ClientShared {
            pool: Arc::new(Mutex::new(ClientPool::default())),
            jar: Arc::new(Mutex::new(CookieJar::default())),
            dns: Arc::new(Mutex::new(DnsCache::default())),
            limits: Arc::new(OriginLimits::default()),
        }),
        policy: ClientPolicy::default(),
    };
    client_handles()
        .lock()
        .expect("HTTP client registry lock")
        .insert(id, handle);
    id
}

fn clone_client_with(
    id: i64,
    change: impl FnOnce(&mut ClientPolicy),
) -> Result<i64, JetHttpBridgeError> {
    let mut handles = client_handles()
        .lock()
        .map_err(|_| JetHttpBridgeError::Internal)?;
    let mut handle = handles
        .get(&id)
        .cloned()
        .ok_or(JetHttpBridgeError::Internal)?;
    change(&mut handle.policy);
    let next = next_client_handle();
    handle.namespace = next;
    handles.insert(next, handle);
    Ok(next)
}

pub fn jet_http_client_cookies_impl(id: i64, enabled: bool) -> Result<i64, JetHttpBridgeError> {
    clone_client_with(id, |policy| policy.cookies = enabled)
}

pub fn jet_http_client_redirects_impl(
    id: i64,
    limit: i64,
    same_origin_credentials: bool,
) -> Result<i64, JetHttpBridgeError> {
    let limit = u32::try_from(limit).map_err(|_| JetHttpBridgeError::Redirect)?;
    clone_client_with(id, |policy| {
        policy.redirect_limit = limit;
        policy.same_origin_credentials = same_origin_credentials;
    })
}

pub fn jet_http_client_allow_http_downgrade_impl(
    id: i64,
    allow: bool,
) -> Result<i64, JetHttpBridgeError> {
    clone_client_with(id, |policy| policy.allow_http_downgrade = allow)
}

/// `mode`: 0 = None, 1 = Safe, 2 = Idempotent (D-HTTP-CLIENT2).
pub fn jet_http_client_retries_impl(id: i64, mode: i64) -> Result<i64, JetHttpBridgeError> {
    let retry_policy = match mode {
        0 => RetryPolicy::None,
        1 => RetryPolicy::Safe,
        2 => RetryPolicy::Idempotent,
        _ => return Err(JetHttpBridgeError::Internal),
    };
    clone_client_with(id, |policy| policy.retry_policy = retry_policy)
}

pub fn jet_http_client_proxy_from_environment_impl(id: i64) -> Result<i64, JetHttpBridgeError> {
    clone_client_with(id, |policy| {
        policy.proxy = None;
        policy.use_environment_proxy = true;
    })
}

pub fn jet_http_client_proxy_impl(id: i64, proxy: Option<&str>) -> Result<i64, JetHttpBridgeError> {
    let proxy = proxy
        .map(parse_url)
        .transpose()
        .map_err(|_| JetHttpBridgeError::Proxy)?
        .map(|url| format!("{}://{}:{}", url.scheme, url.host, url.port));
    clone_client_with(id, |policy| {
        policy.proxy = proxy;
        policy.use_environment_proxy = false;
    })
}

pub fn jet_http_client_protocols_impl(
    id: i64,
    http2: bool,
    http11: bool,
    h2c: bool,
) -> Result<i64, JetHttpBridgeError> {
    if !http2 && !http11 {
        return Err(JetHttpBridgeError::Protocol);
    }
    clone_client_with(id, |policy| {
        policy.http2 = http2;
        policy.http11 = http11;
        policy.h2c = h2c;
    })
}

pub fn jet_http_client_decompression_impl(
    id: i64,
    enabled: bool,
) -> Result<i64, JetHttpBridgeError> {
    clone_client_with(id, |policy| policy.decompress = enabled)
}

pub fn jet_http_client_root_certificate_impl(
    id: i64,
    certificate_der: &[u8],
    include_system_roots: bool,
) -> Result<i64, JetHttpBridgeError> {
    if certificate_der.is_empty() || certificate_der.len() > 1024 * 1024 {
        return Err(JetHttpBridgeError::Tls);
    }
    clone_client_with(id, |policy| {
        policy.system_roots = include_system_roots;
        policy.custom_roots.push(certificate_der.to_vec());
    })
}

pub fn jet_http_client_tls_impl(
    id: i64,
    trust_mode: i64,
    custom_ca_pem: &[u8],
    identity_cert_pem: &[u8],
    identity_key_pem: &[u8],
    min_version: i64,
    max_version: i64,
) -> Result<i64, JetHttpBridgeError> {
    if !matches!(trust_mode, 0 | 1 | 2) {
        return Err(JetHttpBridgeError::Tls);
    }
    if min_version > max_version || !matches!(min_version, 12 | 13) || !matches!(max_version, 12 | 13)
    {
        return Err(JetHttpBridgeError::Tls);
    }
    if (identity_cert_pem.is_empty()) != (identity_key_pem.is_empty()) {
        return Err(JetHttpBridgeError::Tls);
    }
    let custom_roots = if matches!(trust_mode, 1 | 2) {
        http_tls_pem_certificates(custom_ca_pem)?
    } else if !custom_ca_pem.is_empty() {
        return Err(JetHttpBridgeError::Tls);
    } else {
        Vec::new()
    };
    if matches!(trust_mode, 1 | 2) && custom_roots.is_empty() {
        return Err(JetHttpBridgeError::Tls);
    }
    if !identity_cert_pem.is_empty() {
        let _ = http_tls_pem_certificates(identity_cert_pem)?;
        let _ = http_tls_private_key(identity_key_pem)?;
    }
    clone_client_with(id, |policy| {
        policy.system_roots = matches!(trust_mode, 0 | 1);
        policy.custom_roots = custom_roots;
        policy.tls_min_version = min_version;
        policy.tls_max_version = max_version;
        policy.tls_identity_cert = identity_cert_pem.to_vec();
        policy.tls_identity_key = identity_key_pem.to_vec();
    })
}

pub fn jet_http_client_timeouts_impl(
    id: i64,
    dns_ms: i64,
    connect_ms: i64,
    tls_ms: i64,
    write_idle_ms: i64,
    first_byte_ms: i64,
    read_idle_ms: i64,
    total_ms: Option<i64>,
) -> Result<i64, JetHttpBridgeError> {
    let dns = validated_timeout("DNS timeout", dns_ms)?;
    let connect = validated_timeout("connect timeout", connect_ms)?;
    let tls = validated_timeout("TLS timeout", tls_ms)?;
    let write = validated_timeout("write timeout", write_idle_ms)?;
    let first = validated_timeout("first byte timeout", first_byte_ms)?;
    let read = validated_timeout("read timeout", read_idle_ms)?;
    let total = total_ms
        .map(|value| validated_timeout("total timeout", value))
        .transpose()?;
    clone_client_with(id, |policy| {
        policy.dns_timeout = dns;
        policy.connect_timeout = connect;
        policy.tls_timeout = tls;
        policy.write_timeout = write;
        policy.first_byte_timeout = first;
        policy.read_timeout = read;
        policy.total_timeout = total;
    })
}

pub fn jet_http_client_drop_impl(id: i64) {
    let _ = client_handles()
        .lock()
        .map(|mut handles| handles.remove(&id));
}

pub fn jet_http_client_send_with_impl(
    id: i64,
    method: &str,
    url: &String,
    headers_flat: &[String],
    body: Option<&[u8]>,
    timeout_ms: Option<i64>,
    connect_timeout_ms: Option<i64>,
    read_timeout_ms: Option<i64>,
    total_timeout_ms: Option<i64>,
    dns_timeout_ms: Option<i64>,
    tls_timeout_ms: Option<i64>,
    write_timeout_ms: Option<i64>,
    first_byte_timeout_ms: Option<i64>,
    redirects: Option<i64>,
    proxy: Option<&str>,
    cookies_flat: &[String],
    form_flat: &[String],
    multipart_flat: &[String],
) -> Result<(i64, i64, Option<i64>, Vec<String>), JetHttpBridgeError> {
    let handle = client_handles()
        .lock()
        .map_err(|_| JetHttpBridgeError::Internal)?
        .get(&id)
        .cloned()
        .ok_or(JetHttpBridgeError::Internal)?;
    let phases = resolve_request_phase_timeouts(
        timeout_ms,
        connect_timeout_ms,
        read_timeout_ms,
        total_timeout_ms,
        dns_timeout_ms,
        tls_timeout_ms,
        write_timeout_ms,
        first_byte_timeout_ms,
        &handle.policy,
        false,
    )?;
    let (redirect_limit, explicit_redirect_limit) = match redirects {
        Some(value) => (
            u32::try_from(value).map_err(|_| JetHttpBridgeError::Redirect)?,
            true,
        ),
        None => (handle.policy.redirect_limit, false),
    };
    let (headers, body) =
        prepare_request_parts(headers_flat, body, cookies_flat, form_flat, multipart_flat);
    let configured_proxy = proxy
        .or(handle.policy.proxy.as_deref())
        .or((!handle.policy.use_environment_proxy).then_some(""));
    send_following_redirects(
        handle.shared.pool.clone(),
        handle.namespace,
        handle.shared.dns.clone(),
        handle.shared.limits.clone(),
        handle.policy.cookies.then(|| handle.shared.jar.clone()),
        handle.policy.decompress,
        method,
        url,
        headers,
        body,
        phases.dns,
        phases.connect,
        phases.tls,
        phases.first_byte,
        phases.read,
        phases.write,
        phases.total,
        redirect_limit,
        explicit_redirect_limit,
        handle.policy.same_origin_credentials,
        handle.policy.allow_http_downgrade,
        handle.policy.retry_policy,
        configured_proxy,
        handle.policy.http2,
        handle.policy.http11,
        handle.policy.h2c,
        handle.policy.tls_settings(),
    )
}

#[derive(Clone)]
struct StoredCookie {
    name: String,
    value: String,
    domain: String,
    site: String,
    path: String,
    host_only: bool,
    secure: bool,
    expires: Option<SystemTime>,
    created: u64,
    same_site: CookieSameSite,
}

#[derive(Clone, Copy)]
enum CookieSameSite {
    Strict,
    Lax,
    None,
}

#[derive(Default)]
struct CookieJar {
    cookies: Vec<StoredCookie>,
    sequence: u64,
}

impl CookieJar {
    fn header(&mut self, url: &ParsedUrl, same_site: bool, safe_method: bool) -> Option<String> {
        let now = SystemTime::now();
        self.cookies
            .retain(|cookie| cookie.expires.is_none_or(|expires| expires > now));
        let mut matching = self
            .cookies
            .iter()
            .filter(|cookie| {
                (!cookie.secure || url.scheme == "https")
                    && match cookie.same_site {
                        CookieSameSite::Strict => same_site,
                        CookieSameSite::Lax => same_site || safe_method,
                        CookieSameSite::None => true,
                    }
                    && if cookie.host_only {
                        url.host == cookie.domain
                    } else {
                        domain_matches(&url.host, &cookie.domain)
                    }
                    && path_matches(&url.target, &cookie.path)
            })
            .collect::<Vec<_>>();
        matching.sort_by_key(|cookie| (std::cmp::Reverse(cookie.path.len()), cookie.created));
        (!matching.is_empty()).then(|| {
            matching
                .into_iter()
                .map(|cookie| format!("{}={}", cookie.name, cookie.value))
                .collect::<Vec<_>>()
                .join("; ")
        })
    }

    fn store(&mut self, url: &ParsedUrl, value: &str) {
        if value.len() > 4096 {
            return;
        }
        let mut parts = value.split(';').map(str::trim);
        let Some((name, cookie_value)) = parts.next().and_then(|pair| pair.split_once('=')) else {
            return;
        };
        if name.is_empty()
            || !name.bytes().all(http_token_byte)
            || cookie_value
                .bytes()
                .any(|byte| byte < 0x21 || matches!(byte, b';' | b',' | 0x7f))
        {
            return;
        }
        let mut domain = url.host.clone();
        let mut host_only = true;
        let mut secure = false;
        let mut path = default_cookie_path(&url.target);
        let mut expires = None;
        let mut max_age = None;
        let mut same_site = CookieSameSite::Lax;
        for attribute in parts {
            let (name, value) = attribute.split_once('=').unwrap_or((attribute, ""));
            match name.to_ascii_lowercase().as_str() {
                "domain" => {
                    let candidate = value.trim_start_matches('.').to_ascii_lowercase();
                    if !valid_cookie_domain(&candidate) || !domain_matches(&url.host, &candidate) {
                        return;
                    }
                    if is_public_suffix(&candidate) || candidate.parse::<std::net::IpAddr>().is_ok() {
                        if candidate != url.host {
                            return;
                        }
                        domain = url.host.clone();
                        host_only = true;
                    } else {
                        domain = candidate;
                        host_only = false;
                    }
                }
                "path"
                    if value.starts_with('/')
                        && !value.bytes().any(|byte| byte < 0x20 || byte == 0x7f) =>
                {
                    path = value.to_string()
                }
                "secure" => secure = true,
                "max-age" => {
                    if let Ok(seconds) = value.parse::<i64>() {
                        max_age = Some(seconds);
                    }
                }
                "expires" => expires = parse_cookie_date(value),
                "samesite" if value.eq_ignore_ascii_case("strict") => {
                    same_site = CookieSameSite::Strict
                }
                "samesite" if value.eq_ignore_ascii_case("none") => {
                    same_site = CookieSameSite::None
                }
                "samesite" if value.eq_ignore_ascii_case("lax") => same_site = CookieSameSite::Lax,
                _ => {}
            }
        }
        if let Some(seconds) = max_age {
            expires = if seconds <= 0 {
                Some(SystemTime::UNIX_EPOCH)
            } else {
                SystemTime::now().checked_add(Duration::from_secs(seconds as u64))
            };
        }
        if secure && url.scheme != "https" {
            return;
        }
        if name.starts_with("__Secure-") && !secure {
            return;
        }
        if name.starts_with("__Host-")
            && (!secure || !host_only || path != "/")
        {
            return;
        }
        if matches!(same_site, CookieSameSite::None) && !secure {
            return;
        }
        let now = SystemTime::now();
        self.cookies
            .retain(|cookie| cookie.expires.is_none_or(|expires| expires > now));
        if !secure
            && self.cookies.iter().any(|cookie| {
                cookie.secure
                    && cookie.name == name
                    && (domain_matches(&domain, &cookie.domain)
                        || domain_matches(&cookie.domain, &domain))
                    && path_matches(&path, &cookie.path)
            })
        {
            return;
        }
        let created = self
            .cookies
            .iter()
            .find(|cookie| cookie.name == name && cookie.domain == domain && cookie.path == path)
            .map(|cookie| cookie.created);
        self.cookies.retain(|cookie| {
            !(cookie.name == name && cookie.domain == domain && cookie.path == path)
        });
        if expires.is_some_and(|time| time <= SystemTime::now()) {
            return;
        }
        if created.is_none() {
            self.sequence = self.sequence.saturating_add(1);
        }
        let site = cookie_site_domain(&domain);
        self.cookies.push(StoredCookie {
            name: name.to_string(),
            value: cookie_value.to_string(),
            domain: domain.clone(),
            site: site.clone(),
            path,
            host_only,
            secure,
            expires,
            created: created.unwrap_or(self.sequence),
            same_site,
        });
        while self.cookies.iter().filter(|cookie| cookie.site == site).count() > 180 {
            if let Some((index, _)) = self
                .cookies
                .iter()
                .enumerate()
                .filter(|(_, cookie)| cookie.site == site)
                .min_by_key(|(index, cookie)| (cookie.created, *index))
            {
                self.cookies.remove(index);
            }
        }
        while self.cookies.len() > 4096 {
            if let Some((index, _)) = self
                .cookies
                .iter()
                .enumerate()
                .min_by_key(|(index, cookie)| (cookie.created, *index))
            {
                self.cookies.remove(index);
            }
        }
    }
}

fn parse_cookie_date(value: &str) -> Option<SystemTime> {
    let tokens = value
        .split(|character: char| !character.is_ascii_alphanumeric() && character != ':')
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    let mut time = None;
    let mut day = None;
    let mut month = None;
    let mut year = None;
    for token in tokens {
        if time.is_none() {
            let parts = token.split(':').collect::<Vec<_>>();
            if parts.len() == 3 {
                if let (Ok(hour), Ok(minute), Ok(second)) = (
                    parts[0].parse::<u32>(),
                    parts[1].parse::<u32>(),
                    parts[2].parse::<u32>(),
                ) {
                    if hour < 24 && minute < 60 && second < 60 {
                        time = Some((hour, minute, second));
                        continue;
                    }
                }
            }
        }
        if month.is_none() {
            month = [
                "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
            ]
            .iter()
            .position(|name| token.len() >= 3 && token[..3].eq_ignore_ascii_case(name))
            .map(|index| index as u32 + 1);
            if month.is_some() {
                continue;
            }
        }
        if token.bytes().all(|byte| byte.is_ascii_digit()) {
            if day.is_none() && token.len() <= 2 {
                if let Ok(value) = token.parse::<u32>() {
                    if (1..=31).contains(&value) {
                        day = Some(value);
                        continue;
                    }
                }
            }
            if year.is_none() && (2..=4).contains(&token.len()) {
                year = token.parse::<i32>().ok();
            }
        }
    }
    let (hour, minute, second) = time?;
    let day = day?;
    let month = month?;
    let year = match year? {
        value @ 0..=69 => value + 2000,
        value @ 70..=99 => value + 1900,
        value => value,
    };
    if year < 1601 || day > days_in_month(year, month) {
        return None;
    }
    let days = days_from_civil(year, month, day);
    if days < 0 {
        return Some(SystemTime::UNIX_EPOCH);
    }
    SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(
        days as u64 * 86_400 + u64::from(hour * 3600 + minute * 60 + second),
    ))
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => 0,
    }
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = year - i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month = month as i32;
    let day = day as i32;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    i64::from(era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468)
}

fn domain_matches(host: &str, domain: &str) -> bool {
    host == domain
        || host.len() > domain.len()
            && host.as_bytes()[host.len() - domain.len() - 1] == b'.'
            && &host[host.len() - domain.len()..] == domain
}

struct PublicSuffixRules {
    exact: Vec<&'static str>,
    wildcard: Vec<&'static str>,
    exception: Vec<&'static str>,
}

fn public_suffix_rules() -> &'static PublicSuffixRules {
    static RULES: OnceLock<PublicSuffixRules> = OnceLock::new();
    RULES.get_or_init(|| {
        let mut exact = Vec::new();
        let mut wildcard = Vec::new();
        let mut exception = Vec::new();
        for rule in HTTP_PUBLIC_SUFFIX_LIST.split_whitespace() {
            if let Some(rule) = rule.strip_prefix("*.") {
                wildcard.push(rule);
            } else if let Some(rule) = rule.strip_prefix('!') {
                exception.push(rule);
            } else {
                exact.push(rule);
            }
        }
        exact.sort_unstable();
        wildcard.sort_unstable();
        exception.sort_unstable();
        PublicSuffixRules { exact, wildcard, exception }
    })
}

fn valid_cookie_domain(domain: &str) -> bool {
    !domain.is_empty()
        && domain.len() <= 253
        && domain.is_ascii()
        && domain.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

fn public_suffix_label_count(domain: &str) -> usize {
    let labels = domain.split('.').collect::<Vec<_>>();
    let rules = public_suffix_rules();
    let mut matched = 1;
    for index in 0..labels.len() {
        let candidate = labels[index..].join(".");
        if rules.exception.binary_search(&candidate.as_str()).is_ok() {
            return labels.len().saturating_sub(index + 1);
        }
        if rules.exact.binary_search(&candidate.as_str()).is_ok() {
            matched = matched.max(labels.len() - index);
        }
        if index > 0 && rules.wildcard.binary_search(&candidate.as_str()).is_ok() {
            matched = matched.max(labels.len() - index + 1);
        }
    }
    matched
}

fn registrable_domain(domain: &str) -> Option<String> {
    if domain.parse::<std::net::IpAddr>().is_ok() || !valid_cookie_domain(domain) {
        return None;
    }
    let labels = domain.split('.').collect::<Vec<_>>();
    let suffix = public_suffix_label_count(domain);
    (labels.len() > suffix).then(|| labels[labels.len() - suffix - 1..].join("."))
}

fn is_public_suffix(domain: &str) -> bool {
    valid_cookie_domain(domain)
        && domain.parse::<std::net::IpAddr>().is_err()
        && registrable_domain(domain).is_none()
}

fn cookie_site_domain(domain: &str) -> String {
    registrable_domain(domain).unwrap_or_else(|| domain.to_string())
}

fn schemeful_site(url: &ParsedUrl) -> (String, String) {
    (url.scheme.clone(), cookie_site_domain(&url.host))
}

fn default_cookie_path(target: &str) -> String {
    let path = target.split('?').next().unwrap_or("/");
    if !path.starts_with('/') || path == "/" {
        return "/".to_string();
    }
    path.rsplit_once('/')
        .map(|(prefix, _)| {
            if prefix.is_empty() {
                "/".to_string()
            } else {
                prefix.to_string()
            }
        })
        .unwrap_or_else(|| "/".to_string())
}

fn path_matches(request: &str, cookie: &str) -> bool {
    let request = request.split('?').next().unwrap_or("/");
    request == cookie
        || request.starts_with(cookie)
            && (cookie.ends_with('/') || request.as_bytes().get(cookie.len()) == Some(&b'/'))
}

enum HttpStream {
    Plain(TcpStream),
    Tls(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
    H2(Arc<Mutex<H2Connection>>),
}

impl Read for HttpStream {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(stream) => stream.read(out),
            Self::Tls(stream) => stream.read(out),
            Self::H2(_) => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "HTTP/2 uses frames",
            )),
        }
    }
}

impl Write for HttpStream {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(stream) => stream.write(bytes),
            Self::Tls(stream) => stream.write(bytes),
            Self::H2(_) => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "HTTP/2 uses frames",
            )),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Plain(stream) => stream.flush(),
            Self::Tls(stream) => stream.flush(),
            Self::H2(_) => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "HTTP/2 uses frames",
            )),
        }
    }
}

impl HttpStream {
    fn peer_addr(&self) -> Option<SocketAddr> {
        match self {
            Self::Plain(stream) => stream.peer_addr().ok(),
            Self::Tls(stream) => stream.get_ref().peer_addr().ok(),
            Self::H2(connection) => connection
                .lock()
                .ok()
                .and_then(|connection| connection.io.peer_addr()),
        }
    }

    fn is_h2(&self) -> bool {
        matches!(self, Self::H2(_))
    }
}

struct H2Frame {
    kind: u8,
    flags: u8,
    stream: u32,
    payload: Vec<u8>,
}

struct H2Connection {
    io: HttpStream,
    next_stream: u32,
    decoder: HpackDecoder,
    pending: VecDeque<H2Frame>,
    connection_send_window: i64,
    initial_send_window: i64,
    stream_send_windows: HashMap<u32, i64>,
    max_frame: usize,
    streams: HashMap<u32, VecDeque<H2Frame>>,
    active_streams: std::collections::HashSet<u32>,
}

impl H2Connection {
    fn new(mut io: HttpStream) -> Result<Self, JetHttpBridgeError> {
        io.write_all(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n")
            .map_err(map_h2_io)?;
        h2_write_frame(&mut io, 4, 0, 0, &[])?;
        io.flush().map_err(map_h2_io)?;
        Ok(Self {
            io,
            next_stream: 1,
            decoder: HpackDecoder::new(),
            pending: VecDeque::new(),
            connection_send_window: 65_535,
            initial_send_window: 65_535,
            stream_send_windows: HashMap::new(),
            max_frame: 16_384,
            streams: HashMap::new(),
            active_streams: std::collections::HashSet::new(),
        })
    }

    fn control(&mut self, frame: &H2Frame) -> Result<bool, JetHttpBridgeError> {
        match frame.kind {
            4 => {
                if frame.stream != 0
                    || frame.flags & 1 != 0 && !frame.payload.is_empty()
                    || frame.payload.len() % 6 != 0
                {
                    return Err(JetHttpBridgeError::Protocol);
                }
                if frame.flags & 1 == 0 {
                    for setting in frame.payload.chunks_exact(6) {
                        let id = u16::from_be_bytes([setting[0], setting[1]]);
                        let value = u32::from_be_bytes(
                            setting[2..]
                                .try_into()
                                .map_err(|_| JetHttpBridgeError::Protocol)?,
                        );
                        match id {
                            4 if value <= 0x7fff_ffff => {
                                let next = i64::from(value);
                                let delta = next - self.initial_send_window;
                                self.initial_send_window = next;
                                for window in self.stream_send_windows.values_mut() {
                                    *window = window
                                        .checked_add(delta)
                                        .ok_or(JetHttpBridgeError::Protocol)?;
                                }
                            }
                            4 => return Err(JetHttpBridgeError::Protocol),
                            5 if (16_384..=16_777_215).contains(&value) => {
                                self.max_frame = value as usize
                            }
                            5 => return Err(JetHttpBridgeError::Protocol),
                            _ => {}
                        }
                    }
                    h2_write_frame(&mut self.io, 4, 1, 0, &[])?;
                    self.io.flush().map_err(map_h2_io)?;
                }
                Ok(true)
            }
            6 => {
                if frame.stream != 0 || frame.payload.len() != 8 {
                    return Err(JetHttpBridgeError::Protocol);
                }
                if frame.flags & 1 == 0 {
                    h2_write_frame(&mut self.io, 6, 1, 0, &frame.payload)?;
                }
                Ok(true)
            }
            7 => Err(JetHttpBridgeError::Protocol),
            8 => {
                if frame.payload.len() != 4 {
                    return Err(JetHttpBridgeError::Protocol);
                }
                let amount = u32::from_be_bytes(
                    frame.payload[..4]
                        .try_into()
                        .map_err(|_| JetHttpBridgeError::Protocol)?,
                ) & 0x7fff_ffff;
                if amount == 0 {
                    return Err(JetHttpBridgeError::Protocol);
                }
                if frame.stream == 0 {
                    self.connection_send_window = self
                        .connection_send_window
                        .checked_add(i64::from(amount))
                        .filter(|value| *value <= 0x7fff_ffff)
                        .ok_or(JetHttpBridgeError::Protocol)?;
                } else if let Some(window) = self.stream_send_windows.get_mut(&frame.stream) {
                    *window = window
                        .checked_add(i64::from(amount))
                        .filter(|value| *value <= 0x7fff_ffff)
                        .ok_or(JetHttpBridgeError::Protocol)?;
                }
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn probe(&mut self) -> Result<(), JetHttpBridgeError> {
        const PAYLOAD: &[u8; 8] = b"JETPING!";
        h2_write_frame(&mut self.io, 6, 0, 0, PAYLOAD)?;
        self.io.flush().map_err(map_h2_io)?;
        loop {
            let frame = h2_read_frame(&mut self.io)?;
            if frame.kind == 6 && frame.flags & 1 != 0 && frame.payload == PAYLOAD {
                return Ok(());
            }
            if self.control(&frame)? {
                continue;
            }
            if frame.stream == 0 {
                self.pending.push_back(frame);
            } else {
                self.streams
                    .entry(frame.stream)
                    .or_default()
                    .push_back(frame);
            }
        }
    }

    fn start_request(
        &mut self,
        method: &str,
        url: &ParsedUrl,
        headers: &[(String, String)],
        body_len: Option<usize>,
        has_body: bool,
        body_read: &mut dyn FnMut() -> Result<Option<Vec<u8>>, JetHttpBridgeError>,
        tee: &mut Option<Vec<u8>>,
        decompress: bool,
        facts: &Arc<Mutex<ResponseFacts>>,
    ) -> Result<u32, JetHttpBridgeError> {
        let write_started = Instant::now();
        let stream = self.next_stream;
        self.next_stream = self
            .next_stream
            .checked_add(2)
            .filter(|id| *id <= 0x7fff_ffff)
            .ok_or(JetHttpBridgeError::Protocol)?;
        let block = hpack_request(method, url, headers, body_len, decompress)?;
        self.stream_send_windows
            .insert(stream, self.initial_send_window);
        let end_stream = !has_body || body_len == Some(0);
        h2_write_frame(
            &mut self.io,
            1,
            4 | if end_stream { 1 } else { 0 },
            stream,
            &block,
        )?;
        if has_body && body_len != Some(0) {
            let mut pending = Vec::new();
            let mut finished = false;
            while !finished || !pending.is_empty() {
                while self.connection_send_window <= 0
                    || self.stream_send_windows.get(&stream).copied().unwrap_or(0) <= 0
                {
                    let frame = h2_read_frame(&mut self.io)?;
                    if !self.control(&frame)? {
                        self.pending.push_back(frame);
                    }
                }
                if pending.is_empty() && !finished {
                    match body_read()? {
                        Some(chunk) if !chunk.is_empty() => {
                            tee_upload_chunk(tee, &chunk)?;
                            pending = chunk;
                        }
                        Some(_) => {}
                        None => finished = true,
                    }
                }
                if pending.is_empty() {
                    if finished {
                        h2_write_frame(&mut self.io, 0, 1, stream, &[])?;
                    }
                    break;
                }
                let count = pending
                    .len()
                    .min(self.max_frame)
                    .min(HTTP_UPLOAD_CHUNK)
                    .min(self.connection_send_window as usize)
                    .min(self.stream_send_windows[&stream] as usize);
                self.connection_send_window -= count as i64;
                *self
                    .stream_send_windows
                    .get_mut(&stream)
                    .ok_or(JetHttpBridgeError::Protocol)? -= count as i64;
                let chunk = pending.drain(..count).collect::<Vec<_>>();
                let flags = if finished && pending.is_empty() { 1 } else { 0 };
                h2_write_frame(&mut self.io, 0, flags, stream, &chunk)?;
                if flags == 1 {
                    break;
                }
            }
        }
        self.stream_send_windows.remove(&stream);
        self.active_streams.insert(stream);
        self.io.flush().map_err(map_h2_io)?;
        set_timing(facts, 3, elapsed_ms(write_started));
        Ok(stream)
    }

    fn poll_response_headers(
        &mut self,
        stream: u32,
    ) -> Result<Option<(i64, Vec<(String, String)>, bool)>, JetHttpBridgeError> {
        loop {
            let frame = match self
                .streams
                .get_mut(&stream)
                .and_then(VecDeque::pop_front)
                .or_else(|| self.pending.pop_front())
            {
                Some(frame) => frame,
                None => match h2_read_frame(&mut self.io) {
                    Ok(frame) => frame,
                    Err(JetHttpBridgeError::Timeout) => return Ok(None),
                    Err(error) => return Err(error),
                },
            };
            if self.control(&frame)? {
                continue;
            }
            if frame.stream != stream {
                self.streams
                    .entry(frame.stream)
                    .or_default()
                    .push_back(frame);
                continue;
            }
            if frame.kind == 3 {
                self.active_streams.remove(&stream);
                return Err(JetHttpBridgeError::Protocol);
            }
            if frame.kind != 1 {
                self.pending.push_back(frame);
                continue;
            }
            let (block, end) = h2_header_block(&mut self.io, frame)?;
            let decoded = self.decoder.decode(&block)?;
            let (status, headers) = h2_response_headers(decoded)?;
            if status < 200 {
                continue;
            }
            return Ok(Some((status, headers, end)));
        }
    }
}

fn map_h2_io(error: std::io::Error) -> JetHttpBridgeError {
    if matches!(
        error.kind(),
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
    ) {
        JetHttpBridgeError::Timeout
    } else {
        JetHttpBridgeError::Io
    }
}

fn h2_write_frame(
    io: &mut HttpStream,
    kind: u8,
    flags: u8,
    stream: u32,
    payload: &[u8],
) -> Result<(), JetHttpBridgeError> {
    if payload.len() > 0x00ff_ffff || stream > 0x7fff_ffff {
        return Err(JetHttpBridgeError::Protocol);
    }
    let length = payload.len() as u32;
    let mut head = [0u8; 9];
    head[..4].copy_from_slice(&length.to_be_bytes());
    head.copy_within(1..4, 0);
    head[3] = kind;
    head[4] = flags;
    head[5..].copy_from_slice(&(stream & 0x7fff_ffff).to_be_bytes());
    io.write_all(&head)
        .and_then(|_| io.write_all(payload))
        .map_err(map_h2_io)
}

fn h2_read_frame(io: &mut HttpStream) -> Result<H2Frame, JetHttpBridgeError> {
    let mut head = [0u8; 9];
    io.read_exact(&mut head).map_err(map_h2_io)?;
    let length = usize::from(head[0]) << 16 | usize::from(head[1]) << 8 | usize::from(head[2]);
    if length > 64 * 1024 {
        return Err(JetHttpBridgeError::Protocol);
    }
    let stream = u32::from_be_bytes(
        head[5..]
            .try_into()
            .map_err(|_| JetHttpBridgeError::Protocol)?,
    ) & 0x7fff_ffff;
    let mut payload = vec![0; length];
    io.read_exact(&mut payload).map_err(map_h2_io)?;
    Ok(H2Frame {
        kind: head[3],
        flags: head[4],
        stream,
        payload,
    })
}

fn h2_header_block(
    io: &mut HttpStream,
    first: H2Frame,
) -> Result<(Vec<u8>, bool), JetHttpBridgeError> {
    let stream = first.stream;
    let end = first.flags & 1 != 0;
    let mut complete = first.flags & 4 != 0;
    let mut block = first.payload;
    if first.flags & 8 != 0 {
        let padding = usize::from(*block.first().ok_or(JetHttpBridgeError::Protocol)?);
        block.remove(0);
        block.truncate(
            block
                .len()
                .checked_sub(padding)
                .ok_or(JetHttpBridgeError::Protocol)?,
        );
    }
    while !complete && block.len() <= 64 * 1024 {
        let frame = h2_read_frame(io)?;
        if frame.kind != 9 || frame.stream != stream {
            return Err(JetHttpBridgeError::Protocol);
        }
        complete = frame.flags & 4 != 0;
        block.extend_from_slice(&frame.payload);
    }
    if !complete || block.len() > 64 * 1024 {
        return Err(JetHttpBridgeError::InvalidHeader);
    }
    Ok((block, end))
}

fn h2_response_headers(
    decoded: Vec<(String, String)>,
) -> Result<(i64, Vec<(String, String)>), JetHttpBridgeError> {
    let mut status = None;
    let mut regular = false;
    let mut out = Vec::new();
    for (name, value) in decoded {
        if name.bytes().any(|byte| byte.is_ascii_uppercase()) {
            return Err(JetHttpBridgeError::InvalidHeader);
        }
        if name.starts_with(':') {
            if regular || name != ":status" || status.is_some() {
                return Err(JetHttpBridgeError::Protocol);
            }
            let value = value
                .parse::<i64>()
                .map_err(|_| JetHttpBridgeError::Protocol)?;
            if !(100..=599).contains(&value) {
                return Err(JetHttpBridgeError::Protocol);
            }
            status = Some(value);
        } else {
            regular = true;
            if matches!(
                name.as_str(),
                "connection" | "proxy-connection" | "keep-alive" | "upgrade" | "transfer-encoding"
            ) {
                return Err(JetHttpBridgeError::Protocol);
            }
            out.push((name, value));
        }
    }
    Ok((status.ok_or(JetHttpBridgeError::Protocol)?, out))
}

#[derive(Clone, Debug)]
struct ParsedUrl {
    scheme: String,
    host: String,
    port: u16,
    authority: String,
    target: String,
}

fn parse_url(url: &str) -> Result<ParsedUrl, JetHttpBridgeError> {
    let (scheme, rest) = url
        .split_once("://")
        .ok_or(JetHttpBridgeError::InvalidUrl)?;
    if !matches!(scheme, "http" | "https") || rest.is_empty() {
        return Err(JetHttpBridgeError::InvalidUrl);
    }
    let split = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..split];
    if authority.is_empty() || authority.contains('@') || rest[split..].contains('#') {
        return Err(JetHttpBridgeError::InvalidUrl);
    }
    let (host, port) = if let Some(tail) = authority.strip_prefix('[') {
        let close = tail.find(']').ok_or(JetHttpBridgeError::InvalidUrl)?;
        let host = &tail[..close];
        let suffix = &tail[close + 1..];
        let port = if suffix.is_empty() {
            None
        } else {
            Some(
                suffix
                    .strip_prefix(':')
                    .ok_or(JetHttpBridgeError::InvalidUrl)?,
            )
        };
        (host, port)
    } else {
        match authority.rsplit_once(':') {
            Some((host, port)) if !host.contains(':') => (host, Some(port)),
            _ => (authority, None),
        }
    };
    if host.is_empty()
        || host
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
    {
        return Err(JetHttpBridgeError::InvalidUrl);
    }
    let port = port
        .map(|value| {
            value
                .parse::<u16>()
                .map_err(|_| JetHttpBridgeError::InvalidUrl)
        })
        .transpose()?
        .unwrap_or(if scheme == "https" { 443 } else { 80 });
    let target = match &rest[split..] {
        "" => "/".to_string(),
        value if value.starts_with('?') => format!("/{value}"),
        value if value.starts_with('/') => value.to_string(),
        _ => return Err(JetHttpBridgeError::InvalidUrl),
    };
    Ok(ParsedUrl {
        scheme: scheme.to_string(),
        host: host.to_ascii_lowercase(),
        port,
        authority: authority.to_string(),
        target,
    })
}

fn resolve(url: &ParsedUrl) -> Result<Vec<SocketAddr>, JetHttpBridgeError> {
    (url.host.as_str(), url.port)
        .to_socket_addrs()
        .map(|addresses| addresses.collect())
        .map_err(|_| JetHttpBridgeError::Resolve)
}

#[derive(Clone)]
struct DnsEntry {
    result: Result<Vec<SocketAddr>, JetHttpBridgeError>,
    expires: Instant,
}

#[derive(Default)]
struct DnsCache {
    entries: HashMap<(String, u16), DnsEntry>,
}

impl DnsCache {
    fn get(&self, key: &(String, u16)) -> Option<Result<Vec<SocketAddr>, JetHttpBridgeError>> {
        self.entries
            .get(key)
            .filter(|entry| entry.expires > Instant::now())
            .map(|entry| entry.result.clone())
    }

    fn put(&mut self, key: (String, u16), result: Result<Vec<SocketAddr>, JetHttpBridgeError>) {
        let ttl = if result.is_ok() {
            Duration::from_secs(60)
        } else {
            Duration::from_secs(5)
        };
        self.entries.insert(
            key,
            DnsEntry {
                result,
                expires: Instant::now() + ttl,
            },
        );
    }
}

fn default_dns_cache() -> &'static Arc<Mutex<DnsCache>> {
    static CACHE: OnceLock<Arc<Mutex<DnsCache>>> = OnceLock::new();
    CACHE.get_or_init(|| Arc::new(Mutex::new(DnsCache::default())))
}

fn connect_plain(
    dns: &Arc<Mutex<DnsCache>>,
    facts: &Arc<Mutex<ResponseFacts>>,
    url: &ParsedUrl,
    dns_timeout: Duration,
    timeout: Duration,
) -> Result<TcpStream, JetHttpBridgeError> {
    let mut last = JetHttpBridgeError::Connect;
    let dns_started = Instant::now();
    let key = (url.host.clone(), url.port);
    let cached = dns
        .lock()
        .map_err(|_| JetHttpBridgeError::Internal)?
        .get(&key);
    let cache_miss = cached.is_none();
    let result = if let Some(cached) = cached {
        cached
    } else {
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        let target = url.clone();
        std::thread::spawn(move || {
            let result = resolve(&target).and_then(|addresses| {
                (!addresses.is_empty())
                    .then_some(addresses)
                    .ok_or(JetHttpBridgeError::Resolve)
            });
            let _ = send.send(result);
        });
        receive
            .recv_timeout(dns_timeout)
            .map_err(|_| JetHttpBridgeError::Timeout)?
    };
    if cache_miss {
        dns.lock()
            .map_err(|_| JetHttpBridgeError::Internal)?
            .put(key, result.clone());
    }
    set_timing(facts, 0, elapsed_ms(dns_started));
    let addresses = result?;
    let connect_started = Instant::now();
    for address in addresses {
        match TcpStream::connect_timeout(&address, timeout) {
            Ok(stream) => {
                set_timing(facts, 1, elapsed_ms(connect_started));
                return Ok(stream);
            }
            Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {
                last = JetHttpBridgeError::Timeout;
            }
            Err(_) => last = JetHttpBridgeError::Connect,
        }
    }
    set_timing(facts, 1, elapsed_ms(connect_started));
    Err(last)
}

fn http_tls_pem_certificates(pem: &[u8]) -> Result<Vec<Vec<u8>>, JetHttpBridgeError> {
    const BEGIN: &str = "-----BEGIN CERTIFICATE-----";
    const END: &str = "-----END CERTIFICATE-----";
    let text = std::str::from_utf8(pem).map_err(|_| JetHttpBridgeError::Tls)?;
    let mut rest = text;
    let mut out = Vec::new();
    while let Some(start) = rest.find(BEGIN) {
        if !rest[..start].trim().is_empty() {
            return Err(JetHttpBridgeError::Tls);
        }
        rest = &rest[start + BEGIN.len()..];
        let stop = rest.find(END).ok_or(JetHttpBridgeError::Tls)?;
        out.push(http_tls_pem_base64(&rest[..stop])?);
        rest = &rest[stop + END.len()..];
    }
    if out.is_empty() || !rest.trim().is_empty() {
        return Err(JetHttpBridgeError::Tls);
    }
    Ok(out)
}

fn http_tls_private_key(
    pem: &[u8],
) -> Result<rustls::pki_types::PrivateKeyDer<'static>, JetHttpBridgeError> {
    let text = std::str::from_utf8(pem).map_err(|_| JetHttpBridgeError::Tls)?;
    let parse = |begin: &str, end: &str| -> Result<Option<Vec<u8>>, JetHttpBridgeError> {
        let Some(start) = text.find(begin) else {
            return Ok(None);
        };
        if !text[..start].trim().is_empty() {
            return Err(JetHttpBridgeError::Tls);
        }
        let rest = &text[start + begin.len()..];
        let stop = rest.find(end).ok_or(JetHttpBridgeError::Tls)?;
        if !rest[stop + end.len()..].trim().is_empty() {
            return Err(JetHttpBridgeError::Tls);
        }
        Ok(Some(http_tls_pem_base64(&rest[..stop])?))
    };
    if let Some(der) = parse("-----BEGIN PRIVATE KEY-----", "-----END PRIVATE KEY-----")? {
        return Ok(rustls::pki_types::PrivateKeyDer::Pkcs8(
            rustls::pki_types::PrivatePkcs8KeyDer::from(der),
        ));
    }
    if let Some(der) = parse("-----BEGIN RSA PRIVATE KEY-----", "-----END RSA PRIVATE KEY-----")? {
        return Ok(rustls::pki_types::PrivateKeyDer::Pkcs1(
            rustls::pki_types::PrivatePkcs1KeyDer::from(der),
        ));
    }
    if let Some(der) = parse("-----BEGIN EC PRIVATE KEY-----", "-----END EC PRIVATE KEY-----")? {
        return Ok(rustls::pki_types::PrivateKeyDer::Sec1(
            rustls::pki_types::PrivateSec1KeyDer::from(der),
        ));
    }
    Err(JetHttpBridgeError::Tls)
}

fn http_tls_pem_base64(text: &str) -> Result<Vec<u8>, JetHttpBridgeError> {
    let filtered: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if filtered.len() % 4 != 0 {
        return Err(JetHttpBridgeError::Tls);
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < filtered.len() {
        let mut vals = [0u8; 4];
        let mut pad = 0usize;
        for j in 0..4 {
            let byte = filtered[i + j];
            vals[j] = match byte {
                b'A'..=b'Z' => byte - b'A',
                b'a'..=b'z' => byte - b'a' + 26,
                b'0'..=b'9' => byte - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' => {
                    pad += 1;
                    0
                }
                _ => return Err(JetHttpBridgeError::Tls),
            };
            if byte == b'=' && j < 2 {
                return Err(JetHttpBridgeError::Tls);
            }
        }
        if pad > 2 {
            return Err(JetHttpBridgeError::Tls);
        }
        out.push((vals[0] << 2) | (vals[1] >> 4));
        if pad < 2 {
            out.push(((vals[1] & 0x0f) << 4) | (vals[2] >> 2));
        }
        if pad < 1 {
            out.push(((vals[2] & 0x03) << 6) | vals[3]);
        }
        i += 4;
    }
    Ok(out)
}

fn tls_stream(
    tcp: TcpStream,
    host: &str,
    timeout: Duration,
    http2: bool,
    http11: bool,
    tls: TlsSettings<'_>,
) -> Result<HttpStream, JetHttpBridgeError> {
    static PROVIDER: std::sync::Once = std::sync::Once::new();
    PROVIDER.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
    tcp.set_read_timeout(Some(timeout))
        .map_err(|_| JetHttpBridgeError::Tls)?;
    tcp.set_write_timeout(Some(timeout))
        .map_err(|_| JetHttpBridgeError::Tls)?;
    let mut roots = rustls::RootCertStore::empty();
    if tls.system_roots {
        let native =
            rustls_native_certs::load_native_certs().map_err(|_| JetHttpBridgeError::Tls)?;
        for cert in native {
            roots.add(cert).map_err(|_| JetHttpBridgeError::Tls)?;
        }
    }
    for cert in tls.custom_roots {
        roots
            .add(rustls::pki_types::CertificateDer::from(cert.clone()))
            .map_err(|_| JetHttpBridgeError::Tls)?;
    }
    if roots.is_empty() {
        return Err(JetHttpBridgeError::Tls);
    }
    if tls.min_version > tls.max_version
        || !matches!(tls.min_version, 12 | 13)
        || !matches!(tls.max_version, 12 | 13)
    {
        return Err(JetHttpBridgeError::Tls);
    }
    let versions: &[&'static rustls::SupportedProtocolVersion] = match (tls.min_version, tls.max_version) {
        (12, 12) => &[&rustls::version::TLS12],
        (13, 13) => &[&rustls::version::TLS13],
        _ => &[&rustls::version::TLS13, &rustls::version::TLS12],
    };
    let builder = rustls::ClientConfig::builder_with_protocol_versions(versions)
        .with_root_certificates(roots);
    let mut config = if tls.identity_cert.is_empty() {
        builder.with_no_client_auth()
    } else {
        let certs = http_tls_pem_certificates(tls.identity_cert)?
            .into_iter()
            .map(rustls::pki_types::CertificateDer::from)
            .collect::<Vec<_>>();
        let key = http_tls_private_key(tls.identity_key)?;
        builder
            .with_client_auth_cert(certs, key)
            .map_err(|_| JetHttpBridgeError::Tls)?
    };
    config.alpn_protocols = [
        http2.then(|| b"h2".to_vec()),
        http11.then(|| b"http/1.1".to_vec()),
    ]
    .into_iter()
    .flatten()
    .collect();
    let name = rustls::pki_types::ServerName::try_from(host.to_string())
        .map_err(|_| JetHttpBridgeError::Tls)?;
    let mut connection = rustls::ClientConnection::new(Arc::new(config), name)
        .map_err(|_| JetHttpBridgeError::Tls)?;
    let mut tcp = tcp;
    while connection.is_handshaking() {
        connection
            .complete_io(&mut tcp)
            .map_err(|_| JetHttpBridgeError::Tls)?;
    }
    let selected = connection.alpn_protocol().map(<[u8]>::to_vec);
    let stream = HttpStream::Tls(Box::new(rustls::StreamOwned::new(connection, tcp)));
    match selected.as_deref() {
        Some(b"h2") if http2 => Ok(HttpStream::H2(Arc::new(Mutex::new(H2Connection::new(
            stream,
        )?)))),
        Some(b"http/1.1") | None if http11 => Ok(stream),
        _ => Err(JetHttpBridgeError::Protocol),
    }
}

fn body_readers() -> &'static Mutex<std::collections::HashMap<i64, BodyReader>> {
    static READERS: OnceLock<Mutex<std::collections::HashMap<i64, BodyReader>>> = OnceLock::new();
    READERS.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

fn register_body(reader: Box<dyn Read + Send>) -> i64 {
    static NEXT: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);
    let handle = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    body_readers()
        .lock()
        .expect("HTTP body registry lock")
        .insert(handle, Arc::new(Mutex::new(reader)));
    handle
}

pub fn jet_http_client_body_read_impl(
    handle: i64,
    max_chunk: usize,
) -> Result<Option<Vec<u8>>, JetHttpBridgeError> {
    let reader = body_readers()
        .lock()
        .map_err(|_| JetHttpBridgeError::Internal)?
        .get(&handle)
        .cloned()
        .ok_or(JetHttpBridgeError::Internal)?;
    let mut chunk = vec![0; max_chunk];
    let read = reader
        .lock()
        .map_err(|_| JetHttpBridgeError::Internal)?
        .read(&mut chunk)
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::InvalidData => JetHttpBridgeError::InvalidFraming,
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => {
                JetHttpBridgeError::Timeout
            }
            _ => JetHttpBridgeError::Io,
        })?;
    if read == 0 {
        let _ = body_readers()
            .lock()
            .map(|mut readers| readers.remove(&handle));
        return Ok(None);
    }
    chunk.truncate(read);
    Ok(Some(chunk))
}

pub fn jet_http_client_body_close_impl(handle: i64) {
    let _ = body_readers()
        .lock()
        .map(|mut readers| readers.remove(&handle));
}

/// Open response-body registry size — bridge harness leak oracle.
pub fn jet_http_client_open_body_count_impl() -> usize {
    body_readers()
        .lock()
        .map(|readers| readers.len())
        .unwrap_or(0)
}

fn validated_timeout(name: &str, milliseconds: i64) -> Result<Duration, JetHttpBridgeError> {
    let milliseconds = u64::try_from(milliseconds).map_err(|_| {
        let _ = name;
        JetHttpBridgeError::Timeout
    })?;
    Ok(Duration::from_millis(milliseconds))
}

struct PhaseTimeouts {
    dns: Duration,
    connect: Duration,
    tls: Duration,
    write: Duration,
    first_byte: Duration,
    read: Duration,
    total: Option<Instant>,
}

fn resolve_phase_timeout(
    explicit: Option<i64>,
    name: &str,
    blanket: Option<Duration>,
    policy: Duration,
) -> Result<Duration, JetHttpBridgeError> {
    if let Some(ms) = explicit {
        return validated_timeout(name, ms);
    }
    Ok(blanket.unwrap_or(policy))
}

fn resolve_request_phase_timeouts(
    blanket_ms: Option<i64>,
    connect_ms: Option<i64>,
    read_ms: Option<i64>,
    total_ms: Option<i64>,
    dns_ms: Option<i64>,
    tls_ms: Option<i64>,
    write_ms: Option<i64>,
    first_byte_ms: Option<i64>,
    policy: &ClientPolicy,
    oneshot_default: bool,
) -> Result<PhaseTimeouts, JetHttpBridgeError> {
    let blanket = if oneshot_default {
        Some(validated_timeout(
            "timeout",
            blanket_ms.unwrap_or(30_000),
        )?)
    } else {
        blanket_ms
            .map(|value| validated_timeout("timeout", value))
            .transpose()?
    };
    let total = compose_total_deadline(match total_ms {
        Some(ms) => Some(validated_timeout("total timeout", ms)?),
        None if oneshot_default => None,
        None => policy.total_timeout,
    })?;
    Ok(PhaseTimeouts {
        dns: resolve_phase_timeout(dns_ms, "DNS timeout", blanket, policy.dns_timeout)?,
        connect: resolve_phase_timeout(
            connect_ms,
            "connect timeout",
            blanket,
            policy.connect_timeout,
        )?,
        tls: resolve_phase_timeout(tls_ms, "TLS timeout", blanket, policy.tls_timeout)?,
        write: resolve_phase_timeout(write_ms, "write timeout", blanket, policy.write_timeout)?,
        first_byte: resolve_phase_timeout(
            first_byte_ms,
            "first byte timeout",
            blanket,
            policy.first_byte_timeout,
        )?,
        read: resolve_phase_timeout(read_ms, "read timeout", blanket, policy.read_timeout)?,
        total,
    })
}

/// Perform an HTTP GET. Returns (status_code, body, headers_flat) where headers_flat
/// is alternating [key, value, key, value, ...].
pub fn jet_http_client_get_impl(
    url: &String,
) -> Result<(i64, i64, Option<i64>, Vec<String>), JetHttpBridgeError> {
    jet_http_client_send_impl(
        "GET",
        url,
        &[],
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        &[],
        &[],
        &[],
    )
}

/// Perform an HTTP POST with a string body.
pub fn jet_http_client_post_impl(
    url: &String,
    body: &String,
) -> Result<(i64, i64, Option<i64>, Vec<String>), JetHttpBridgeError> {
    jet_http_client_send_impl(
        "POST",
        url,
        &[],
        Some(body.as_bytes()),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        &[],
        &[],
        &[],
    )
}

/// Perform a generic HTTP request.
/// headers_flat: alternating [key, value, key, value, ...]

pub fn jet_http_client_send_stream_impl(
    method: &str,
    url: &String,
    headers_flat: &[String],
    body_len: Option<i64>,
    has_user_body: bool,
    body_read: &mut dyn FnMut() -> Result<Option<Vec<u8>>, JetHttpBridgeError>,
    timeout_ms: Option<i64>,
    connect_timeout_ms: Option<i64>,
    read_timeout_ms: Option<i64>,
    total_timeout_ms: Option<i64>,
    dns_timeout_ms: Option<i64>,
    tls_timeout_ms: Option<i64>,
    write_timeout_ms: Option<i64>,
    first_byte_timeout_ms: Option<i64>,
    redirects: Option<i64>,
    proxy: Option<&str>,
    cookies_flat: &[String],
    form_flat: &[String],
    multipart_flat: &[String],
) -> Result<(i64, i64, Option<i64>, Vec<String>), JetHttpBridgeError> {
    let phases = resolve_request_phase_timeouts(
        timeout_ms,
        connect_timeout_ms,
        read_timeout_ms,
        total_timeout_ms,
        dns_timeout_ms,
        tls_timeout_ms,
        write_timeout_ms,
        first_byte_timeout_ms,
        &ClientPolicy::default(),
        true,
    )?;
    let redirects = redirects
        .map(|limit| u32::try_from(limit).map_err(|_| JetHttpBridgeError::Redirect))
        .transpose()?;
    let explicit_redirect_limit = redirects.is_some();
    let redirect_limit = redirects.unwrap_or(HTTP_CLIENT_DEFAULT_REDIRECTS);
    let (headers, form_body) = if has_user_body {
        prepare_request_parts(headers_flat, None, cookies_flat, &[], &[])
    } else {
        prepare_request_parts(headers_flat, None, cookies_flat, form_flat, multipart_flat)
    };
    let stream_len = if has_user_body {
        body_len.and_then(|value| usize::try_from(value).ok())
    } else {
        form_body.as_ref().map(Vec::len)
    };
    send_following_redirects_upload(
        default_client_pool().clone(),
        0,
        default_dns_cache().clone(),
        default_origin_limits().clone(),
        None,
        true,
        method,
        url,
        headers,
        form_body,
        if has_user_body { Some(body_read) } else { None },
        stream_len,
        phases.dns,
        phases.connect,
        phases.tls,
        phases.first_byte,
        phases.read,
        phases.write,
        phases.total,
        redirect_limit,
        explicit_redirect_limit,
        true,
        false,
        RetryPolicy::Safe,
        proxy,
        true,
        true,
        false,
        TlsSettings::SYSTEM,
    )
}

pub fn jet_http_client_send_with_stream_impl(
    id: i64,
    method: &str,
    url: &String,
    headers_flat: &[String],
    body_len: Option<i64>,
    has_user_body: bool,
    body_read: &mut dyn FnMut() -> Result<Option<Vec<u8>>, JetHttpBridgeError>,
    timeout_ms: Option<i64>,
    connect_timeout_ms: Option<i64>,
    read_timeout_ms: Option<i64>,
    total_timeout_ms: Option<i64>,
    dns_timeout_ms: Option<i64>,
    tls_timeout_ms: Option<i64>,
    write_timeout_ms: Option<i64>,
    first_byte_timeout_ms: Option<i64>,
    redirects: Option<i64>,
    proxy: Option<&str>,
    cookies_flat: &[String],
    form_flat: &[String],
    multipart_flat: &[String],
) -> Result<(i64, i64, Option<i64>, Vec<String>), JetHttpBridgeError> {
    let handle = client_handles()
        .lock()
        .map_err(|_| JetHttpBridgeError::Internal)?
        .get(&id)
        .cloned()
        .ok_or(JetHttpBridgeError::Internal)?;
    let phases = resolve_request_phase_timeouts(
        timeout_ms,
        connect_timeout_ms,
        read_timeout_ms,
        total_timeout_ms,
        dns_timeout_ms,
        tls_timeout_ms,
        write_timeout_ms,
        first_byte_timeout_ms,
        &handle.policy,
        false,
    )?;
    let (redirect_limit, explicit_redirect_limit) = match redirects {
        Some(value) => (
            u32::try_from(value).map_err(|_| JetHttpBridgeError::Redirect)?,
            true,
        ),
        None => (handle.policy.redirect_limit, false),
    };
    let (headers, form_body) = if has_user_body {
        prepare_request_parts(headers_flat, None, cookies_flat, &[], &[])
    } else {
        prepare_request_parts(headers_flat, None, cookies_flat, form_flat, multipart_flat)
    };
    let stream_len = if has_user_body {
        body_len.and_then(|value| usize::try_from(value).ok())
    } else {
        form_body.as_ref().map(Vec::len)
    };
    let configured_proxy = proxy
        .or(handle.policy.proxy.as_deref())
        .or((!handle.policy.use_environment_proxy).then_some(""));
    send_following_redirects_upload(
        handle.shared.pool.clone(),
        handle.namespace,
        handle.shared.dns.clone(),
        handle.shared.limits.clone(),
        handle.policy.cookies.then(|| handle.shared.jar.clone()),
        handle.policy.decompress,
        method,
        url,
        headers,
        form_body,
        if has_user_body { Some(body_read) } else { None },
        stream_len,
        phases.dns,
        phases.connect,
        phases.tls,
        phases.first_byte,
        phases.read,
        phases.write,
        phases.total,
        redirect_limit,
        explicit_redirect_limit,
        handle.policy.same_origin_credentials,
        handle.policy.allow_http_downgrade,
        handle.policy.retry_policy,
        configured_proxy,
        handle.policy.http2,
        handle.policy.http11,
        handle.policy.h2c,
        handle.policy.tls_settings(),
    )
}

pub fn jet_http_client_send_impl(
    method: &str,
    url: &String,
    headers_flat: &[String],
    body: Option<&[u8]>,
    timeout_ms: Option<i64>,
    connect_timeout_ms: Option<i64>,
    read_timeout_ms: Option<i64>,
    total_timeout_ms: Option<i64>,
    dns_timeout_ms: Option<i64>,
    tls_timeout_ms: Option<i64>,
    write_timeout_ms: Option<i64>,
    first_byte_timeout_ms: Option<i64>,
    redirects: Option<i64>,
    proxy: Option<&str>,
    cookies_flat: &[String],
    form_flat: &[String],
    multipart_flat: &[String],
) -> Result<(i64, i64, Option<i64>, Vec<String>), JetHttpBridgeError> {
    let phases = resolve_request_phase_timeouts(
        timeout_ms,
        connect_timeout_ms,
        read_timeout_ms,
        total_timeout_ms,
        dns_timeout_ms,
        tls_timeout_ms,
        write_timeout_ms,
        first_byte_timeout_ms,
        &ClientPolicy::default(),
        true,
    )?;
    let redirects = redirects
        .map(|limit| u32::try_from(limit).map_err(|_| JetHttpBridgeError::Redirect))
        .transpose()?;
    let explicit_redirect_limit = redirects.is_some();
    let redirect_limit = redirects.unwrap_or(HTTP_CLIENT_DEFAULT_REDIRECTS);
    let (headers, body) =
        prepare_request_parts(headers_flat, body, cookies_flat, form_flat, multipart_flat);
    send_following_redirects(
        default_client_pool().clone(),
        0,
        default_dns_cache().clone(),
        default_origin_limits().clone(),
        None,
        true,
        method,
        url,
        headers,
        body,
        phases.dns,
        phases.connect,
        phases.tls,
        phases.first_byte,
        phases.read,
        phases.write,
        phases.total,
        redirect_limit,
        explicit_redirect_limit,
        true,
        false,
        RetryPolicy::Safe,
        proxy,
        true,
        true,
        false,
        TlsSettings::SYSTEM,
    )
}

fn prepare_request_parts(
    headers_flat: &[String],
    body: Option<&[u8]>,
    cookies_flat: &[String],
    form_flat: &[String],
    multipart_flat: &[String],
) -> (Vec<(String, String)>, Option<Vec<u8>>) {
    let mut headers = coalesce_request_headers(headers_flat);
    if !cookies_flat.is_empty() {
        let cookie = cookies_flat
            .chunks_exact(2)
            .map(|pair| format!("{}={}", pair[0], pair[1]))
            .collect::<Vec<_>>()
            .join("; ");
        headers.push(("cookie".to_string(), cookie));
    }
    let body = if let Some(body) = body {
        Some(body.to_vec())
    } else if !multipart_flat.is_empty() {
        let boundary = multipart_boundary(multipart_flat);
        headers.push((
            "content-type".to_string(),
            format!("multipart/form-data; boundary={boundary}"),
        ));
        Some(encode_multipart(multipart_flat, &boundary).into_bytes())
    } else if !form_flat.is_empty() {
        headers.push((
            "content-type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        ));
        Some(encode_form(form_flat).into_bytes())
    } else {
        None
    };
    (headers, body)
}

fn send_following_redirects(
    pool: Arc<Mutex<ClientPool>>,
    namespace: i64,
    dns: Arc<Mutex<DnsCache>>,
    limits: Arc<OriginLimits>,
    jar: Option<Arc<Mutex<CookieJar>>>,
    decompress: bool,
    original_method: &str,
    original_url: &str,
    headers: Vec<(String, String)>,
    original_body: Option<Vec<u8>>,
    dns_timeout: Duration,
    connect_timeout: Duration,
    tls_timeout: Duration,
    first_byte_timeout: Duration,
    read_timeout: Duration,
    write_timeout: Duration,
    total_deadline: Option<Instant>,
    redirect_limit: u32,
    explicit_redirect_limit: bool,
    same_origin_credentials: bool,
    allow_http_downgrade: bool,
    retry_policy: RetryPolicy,
    explicit_proxy: Option<&str>,
    http2: bool,
    http11: bool,
    h2c: bool,
    tls: TlsSettings<'_>,
) -> Result<(i64, i64, Option<i64>, Vec<String>), JetHttpBridgeError> {
    send_following_redirects_upload(
        pool,
        namespace,
        dns,
        limits,
        jar,
        decompress,
        original_method,
        original_url,
        headers,
        original_body,
        None,
        None,
        dns_timeout,
        connect_timeout,
        tls_timeout,
        first_byte_timeout,
        read_timeout,
        write_timeout,
        total_deadline,
        redirect_limit,
        explicit_redirect_limit,
        same_origin_credentials,
        allow_http_downgrade,
        retry_policy,
        explicit_proxy,
        http2,
        http11,
        h2c,
        tls,
    )
}

fn send_following_redirects_upload(
    pool: Arc<Mutex<ClientPool>>,
    namespace: i64,
    dns: Arc<Mutex<DnsCache>>,
    limits: Arc<OriginLimits>,
    jar: Option<Arc<Mutex<CookieJar>>>,
    decompress: bool,
    original_method: &str,
    original_url: &str,
    mut headers: Vec<(String, String)>,
    mut body: Option<Vec<u8>>,
    mut body_stream: Option<&mut dyn FnMut() -> Result<Option<Vec<u8>>, JetHttpBridgeError>>,
    stream_len: Option<usize>,
    dns_timeout: Duration,
    connect_timeout: Duration,
    tls_timeout: Duration,
    first_byte_timeout: Duration,
    read_timeout: Duration,
    write_timeout: Duration,
    total_deadline: Option<Instant>,
    redirect_limit: u32,
    explicit_redirect_limit: bool,
    same_origin_credentials: bool,
    allow_http_downgrade: bool,
    retry_policy: RetryPolicy,
    explicit_proxy: Option<&str>,
    http2: bool,
    http11: bool,
    h2c: bool,
    tls: TlsSettings<'_>,
) -> Result<(i64, i64, Option<i64>, Vec<String>), JetHttpBridgeError> {
    let mut method = original_method.to_string();
    let mut url = original_url.to_string();
    let mut visited = std::collections::HashSet::new();
    let mut redirect_history = Vec::new();
    let started = Instant::now();
    let cookie_site = schemeful_site(&parse_url(original_url)?);
    for followed in 0..=redirect_limit {
        if !visited.insert(url.clone()) {
            return Err(JetHttpBridgeError::Redirect);
        }
        let parsed = parse_url(&url)?;
        let proxy = select_proxy(&parsed, explicit_proxy)?;
        let mut request_headers = headers.clone();
        if !request_headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("cookie"))
        {
            let same_site = cookie_site == schemeful_site(&parsed);
            let safe_method = matches!(method.as_str(), "GET" | "HEAD" | "OPTIONS" | "TRACE");
            let allow_jar_cookie = followed == 0 || (same_origin_credentials && same_site);
            if allow_jar_cookie {
                if let Some(cookie) = jar
                    .as_ref()
                    .and_then(|jar| jar.lock().ok()?.header(&parsed, same_site, safe_method))
                {
                    request_headers.push(("cookie".to_string(), cookie));
                }
            }
        }
        let response = if let Some(ref bytes) = body {
            send_once(
                pool.clone(),
                namespace,
                dns.clone(),
                limits.clone(),
                &method,
                &parsed,
                &request_headers,
                Some(bytes.as_slice()),
                dns_timeout,
                connect_timeout,
                tls_timeout,
                first_byte_timeout,
                read_timeout,
                write_timeout,
                total_deadline,
                proxy.as_ref(),
                decompress,
                started,
                http2,
                http11,
                h2c,
                tls,
                retry_policy,
            )?
        } else if let Some(ref mut read) = body_stream {
            // Buffer only when a later redirect may need the body again (307/308
            // preserve method+body for non-safe methods). Connection-retry
            // policy excludes POST and must not gate this tee.
            // redirects(0) and bodyless/safe methods stream with no tee.
            let mut tee = (redirect_limit > 0 && redirect_may_replay_body(method.as_str()))
                .then(Vec::new);
            let chunked = stream_len.is_none();
            let has_body = true;
            let response = send_once_upload(
                pool.clone(),
                namespace,
                dns.clone(),
                limits.clone(),
                &method,
                &parsed,
                &request_headers,
                stream_len,
                chunked,
                has_body,
                read,
                &mut tee,
                dns_timeout,
                connect_timeout,
                tls_timeout,
                first_byte_timeout,
                read_timeout,
                write_timeout,
                total_deadline,
                proxy.as_ref(),
                decompress,
                started,
                http2,
                http11,
                h2c,
                tls,
                retry_policy,
            )?;
            body = tee;
            body_stream = None;
            response
        } else {
            send_once(
                pool.clone(),
                namespace,
                dns.clone(),
                limits.clone(),
                &method,
                &parsed,
                &request_headers,
                None,
                dns_timeout,
                connect_timeout,
                tls_timeout,
                first_byte_timeout,
                read_timeout,
                write_timeout,
                total_deadline,
                proxy.as_ref(),
                decompress,
                started,
                http2,
                http11,
                h2c,
                tls,
                retry_policy,
            )?
        };
        if let Some(jar) = &jar {
            if let Ok(mut jar) = jar.lock() {
                for value in response
                    .headers
                    .iter()
                    .filter(|(name, _)| name == "set-cookie")
                    .map(|(_, value)| value)
                {
                    jar.store(&parsed, value);
                }
            }
        }
        if !matches!(response.status, 301 | 302 | 303 | 307 | 308) {
            return finalize_response(response, redirect_history, started);
        }
        let location = header_first(&response.headers, "location");
        let Some(location) = location else {
            return finalize_response(response, redirect_history, started);
        };
        if followed == redirect_limit
            || (explicit_redirect_limit && redirect_limit != 0 && followed + 1 == redirect_limit)
        {
            return if redirect_limit == 0 {
                finalize_response(response, redirect_history, started)
            } else {
                jet_http_client_body_close_impl(response.body_handle);
                Err(JetHttpBridgeError::Redirect)
            };
        }
        let next = resolve_redirect(&parsed, location)?;
        let next_parsed = parse_url(&next)?;
        if parsed.scheme == "https" && next_parsed.scheme == "http" && !allow_http_downgrade {
            jet_http_client_body_close_impl(response.body_handle);
            return Err(JetHttpBridgeError::Redirect);
        }
        let cross_origin = parsed.host != next_parsed.host
            || parsed.port != next_parsed.port
            || parsed.scheme != next_parsed.scheme;
        // D-HTTP-CLIENT2: cross-origin always strips credentials; same-origin
        // keeps them only when same_origin_credentials is true.
        if cross_origin || !same_origin_credentials {
            headers.retain(|(name, _)| {
                !matches!(
                    name.to_ascii_lowercase().as_str(),
                    "authorization" | "proxy-authorization" | "cookie"
                )
            });
        }
        if matches!(response.status, 301 | 302 | 303) && method == "POST" {
            method = "GET".to_string();
            body = None;
            headers.retain(|(name, _)| {
                !matches!(
                    name.to_ascii_lowercase().as_str(),
                    "content-length" | "content-type"
                )
            });
        } else if matches!(response.status, 307 | 308)
            && body.is_none()
            && !matches!(method.as_str(), "GET" | "HEAD" | "OPTIONS" | "TRACE")
        {
            jet_http_client_body_close_impl(response.body_handle);
            return Err(JetHttpBridgeError::Redirect);
        }
        redirect_history.push(url.clone());
        drain_redirect_body(response.body_handle);
        url = next;
    }
    Err(JetHttpBridgeError::Redirect)
}

fn finalize_response(
    response: NativeResponse,
    redirect_history: Vec<String>,
    started: Instant,
) -> Result<(i64, i64, Option<i64>, Vec<String>), JetHttpBridgeError> {
    let facts = response.facts.clone();
    {
        let mut facts = facts.lock().map_err(|_| JetHttpBridgeError::Internal)?;
        facts.protocol = response.protocol.clone();
        facts.remote_address = response.remote_address.clone();
        facts.redirect_history = redirect_history;
        facts.timings_ms[6] = elapsed_ms(started);
        facts.reused_connection = response.reused_connection;
    }
    let handle = response.body_handle;
    let public = response.into_public();
    response_facts()
        .lock()
        .map_err(|_| JetHttpBridgeError::Internal)?
        .insert(handle, facts);
    Ok(public)
}

fn elapsed_ms(started: Instant) -> i64 {
    started.elapsed().as_millis().min(i64::MAX as u128) as i64
}

fn set_timing(facts: &Arc<Mutex<ResponseFacts>>, index: usize, value: i64) {
    if let Ok(mut facts) = facts.lock() {
        facts.timings_ms[index] = facts.timings_ms[index].saturating_add(value);
    }
}

fn drain_redirect_body(handle: i64) {
    let mut total = 0usize;
    loop {
        match jet_http_client_body_read_impl(handle, 64 * 1024) {
            Ok(Some(bytes)) if total.saturating_add(bytes.len()) <= 1024 * 1024 => {
                total += bytes.len()
            }
            Ok(None) => return,
            _ => {
                jet_http_client_body_close_impl(handle);
                return;
            }
        }
    }
}

fn resolve_redirect(base: &ParsedUrl, location: &str) -> Result<String, JetHttpBridgeError> {
    if location.starts_with("http://") || location.starts_with("https://") {
        parse_url(location)?;
        return Ok(location.to_string());
    }
    if location.starts_with("//") {
        return Ok(format!("{}:{location}", base.scheme));
    }
    let authority = if (base.scheme == "http" && base.port == 80)
        || (base.scheme == "https" && base.port == 443)
    {
        base.host.clone()
    } else {
        format!("{}:{}", base.host, base.port)
    };
    if location.starts_with('/') {
        return Ok(format!("{}://{}{}", base.scheme, authority, location));
    }
    let directory = base
        .target
        .split('?')
        .next()
        .unwrap_or("/")
        .rsplit_once('/')
        .map(|(dir, _)| format!("{dir}/"))
        .unwrap_or_else(|| "/".to_string());
    Ok(format!(
        "{}://{}{}{}",
        base.scheme, authority, directory, location
    ))
}

fn select_proxy(
    url: &ParsedUrl,
    explicit: Option<&str>,
) -> Result<Option<ParsedUrl>, JetHttpBridgeError> {
    if let Some(value) = explicit {
        if value.is_empty() {
            return Ok(None);
        }
        return parse_url(value)
            .map(Some)
            .map_err(|_| JetHttpBridgeError::Proxy);
    }
    let no_proxy = std::env::var("no_proxy")
        .ok()
        .or_else(|| std::env::var("NO_PROXY").ok());
    if no_proxy
        .as_deref()
        .is_some_and(|list| no_proxy_matches(list, &url.host, url.port))
    {
        return Ok(None);
    }
    let names: &[&str] = if url.scheme == "https" {
        &["https_proxy", "HTTPS_PROXY", "all_proxy", "ALL_PROXY"]
    } else {
        &["http_proxy", "HTTP_PROXY", "all_proxy", "ALL_PROXY"]
    };
    for name in names {
        if let Ok(value) = std::env::var(name) {
            if !value.is_empty() {
                return parse_url(&value)
                    .map(Some)
                    .map_err(|_| JetHttpBridgeError::Proxy);
            }
        }
    }
    Ok(None)
}

fn no_proxy_matches(list: &str, host: &str, port: u16) -> bool {
    list.split(',').map(str::trim).any(|entry| {
        if entry == "*" {
            return true;
        }
        let (name, entry_port) = entry
            .rsplit_once(':')
            .and_then(|(name, port)| port.parse::<u16>().ok().map(|port| (name, Some(port))))
            .unwrap_or((entry, None));
        if entry_port.is_some_and(|expected| expected != port) {
            return false;
        }
        let name = name.trim_start_matches('.');
        host.eq_ignore_ascii_case(name)
            || host.len() > name.len()
                && host.as_bytes()[host.len() - name.len() - 1] == b'.'
                && host[host.len() - name.len()..].eq_ignore_ascii_case(name)
    })
}

struct NativeResponse {
    status: i64,
    headers: Vec<(String, String)>,
    body_handle: i64,
    body_length: Option<i64>,
    reused_connection: bool,
    remote_address: String,
    protocol: String,
    facts: Arc<Mutex<ResponseFacts>>,
}

impl NativeResponse {
    fn into_public(self) -> (i64, i64, Option<i64>, Vec<String>) {
        let headers = self
            .headers
            .into_iter()
            .flat_map(|(name, value)| [name, value])
            .collect();
        (self.status, self.body_handle, self.body_length, headers)
    }
}

#[derive(Clone, Default)]
struct ResponseFacts {
    protocol: String,
    remote_address: String,
    redirect_history: Vec<String>,
    // dns, connect, tls, write_idle, first_byte, read_idle, total
    timings_ms: [i64; 7],
    reused_connection: bool,
    raw_content_encoding: Option<String>,
}

fn response_facts() -> &'static Mutex<HashMap<i64, Arc<Mutex<ResponseFacts>>>> {
    static FACTS: OnceLock<Mutex<HashMap<i64, Arc<Mutex<ResponseFacts>>>>> = OnceLock::new();
    FACTS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn jet_http_client_response_protocol_impl(handle: i64) -> String {
    response_facts()
        .lock()
        .ok()
        .and_then(|facts| facts.get(&handle).cloned())
        .and_then(|facts| facts.lock().ok().map(|facts| facts.protocol.clone()))
        .unwrap_or_default()
}

pub fn jet_http_client_response_remote_address_impl(handle: i64) -> String {
    response_facts()
        .lock()
        .ok()
        .and_then(|facts| facts.get(&handle).cloned())
        .and_then(|facts| facts.lock().ok().map(|facts| facts.remote_address.clone()))
        .unwrap_or_default()
}

pub fn jet_http_client_response_redirect_history_impl(handle: i64) -> Vec<String> {
    response_facts()
        .lock()
        .ok()
        .and_then(|facts| facts.get(&handle).cloned())
        .and_then(|facts| {
            facts
                .lock()
                .ok()
                .map(|facts| facts.redirect_history.clone())
        })
        .unwrap_or_default()
}

pub fn jet_http_client_response_timings_impl(handle: i64) -> Vec<i64> {
    response_facts()
        .lock()
        .ok()
        .and_then(|facts| facts.get(&handle).cloned())
        .and_then(|facts| facts.lock().ok().map(|facts| facts.timings_ms.to_vec()))
        .unwrap_or_default()
}

pub fn jet_http_client_response_reused_impl(handle: i64) -> bool {
    response_facts()
        .lock()
        .ok()
        .and_then(|facts| facts.get(&handle).cloned())
        .and_then(|facts| facts.lock().ok().map(|facts| facts.reused_connection))
        .unwrap_or(false)
}

pub fn jet_http_client_response_raw_encoding_impl(handle: i64) -> Option<String> {
    response_facts()
        .lock()
        .ok()
        .and_then(|facts| facts.get(&handle).cloned())
        .and_then(|facts| {
            facts
                .lock()
                .ok()
                .and_then(|facts| facts.raw_content_encoding.clone())
        })
}

pub fn jet_http_client_response_facts_drop_impl(handle: i64) {
    let _ = response_facts()
        .lock()
        .map(|mut facts| facts.remove(&handle));
}

fn remaining_timeout(
    default: Duration,
    deadline: Option<Instant>,
) -> Result<Duration, JetHttpBridgeError> {
    match deadline {
        Some(deadline) => deadline
            .checked_duration_since(Instant::now())
            .map(|remaining| remaining.min(default))
            .ok_or(JetHttpBridgeError::Timeout),
        None => Ok(default),
    }
}

fn send_once(
    pool: Arc<Mutex<ClientPool>>,
    namespace: i64,
    dns: Arc<Mutex<DnsCache>>,
    limits: Arc<OriginLimits>,
    method: &str,
    url: &ParsedUrl,
    headers: &[(String, String)],
    body: Option<&[u8]>,
    dns_timeout: Duration,
    connect_timeout: Duration,
    tls_timeout: Duration,
    first_byte_timeout: Duration,
    read_timeout: Duration,
    write_timeout: Duration,
    total_deadline: Option<Instant>,
    proxy: Option<&ParsedUrl>,
    decompress: bool,
    request_started: Instant,
    http2: bool,
    http11: bool,
    h2c: bool,
    tls: TlsSettings<'_>,
    retry_policy: RetryPolicy,
) -> Result<NativeResponse, JetHttpBridgeError> {
    let body_len = body.map(|bytes| bytes.len());
    let has_body = body.is_some();
    let mut offset = 0usize;
    let bytes = body.unwrap_or(&[]);
    let mut tee = None;
    let mut read = slice_body_reader(bytes, &mut offset);
    send_once_upload(
        pool,
        namespace,
        dns,
        limits,
        method,
        url,
        headers,
        body_len,
        false,
        has_body,
        &mut read,
        &mut tee,
        dns_timeout,
        connect_timeout,
        tls_timeout,
        first_byte_timeout,
        read_timeout,
        write_timeout,
        total_deadline,
        proxy,
        decompress,
        request_started,
        http2,
        http11,
        h2c,
        tls,
        retry_policy,
    )
}

fn send_once_upload(
    pool: Arc<Mutex<ClientPool>>,
    namespace: i64,
    dns: Arc<Mutex<DnsCache>>,
    limits: Arc<OriginLimits>,
    method: &str,
    url: &ParsedUrl,
    headers: &[(String, String)],
    body_len: Option<usize>,
    chunked: bool,
    has_body: bool,
    body_read: &mut dyn FnMut() -> Result<Option<Vec<u8>>, JetHttpBridgeError>,
    tee: &mut Option<Vec<u8>>,
    dns_timeout: Duration,
    connect_timeout: Duration,
    tls_timeout: Duration,
    first_byte_timeout: Duration,
    read_timeout: Duration,
    write_timeout: Duration,
    total_deadline: Option<Instant>,
    proxy: Option<&ParsedUrl>,
    decompress: bool,
    request_started: Instant,
    http2: bool,
    http11: bool,
    h2c: bool,
    tls: TlsSettings<'_>,
    retry_policy: RetryPolicy,
) -> Result<NativeResponse, JetHttpBridgeError> {
    if method.is_empty() || !method.bytes().all(http_token_byte) {
        return Err(JetHttpBridgeError::InvalidHeader);
    }
    for (name, value) in headers {
        if name.is_empty()
            || !name.bytes().all(http_token_byte)
            || value.bytes().any(|byte| matches!(byte, b'\r' | b'\n' | 0))
        {
            return Err(JetHttpBridgeError::InvalidHeader);
        }
    }
    let proxy_key = proxy.map(|proxy| format!("{}://{}:{}", proxy.scheme, proxy.host, proxy.port));
    let base_key = PoolKey {
        namespace,
        scheme: url.scheme.clone(),
        host: url.host.clone(),
        port: url.port,
        proxy: proxy_key,
        protocol: "any",
    };
    let permit = limits.acquire(base_key.clone(), total_deadline)?;
    let mut reused = false;
    let facts = Arc::new(Mutex::new(ResponseFacts::default()));
    let mut h2_key = base_key.clone();
    h2_key.protocol = "h2";
    let mut h1_key = base_key;
    h1_key.protocol = "http/1.1";
    let mut pool_guard = pool.lock().map_err(|_| JetHttpBridgeError::Internal)?;
    let mut stream = if http2 {
        pool_guard.take(&h2_key)
    } else {
        None
    };
    if stream.is_none() && http11 {
        stream = pool_guard.take(&h1_key);
    }
    drop(pool_guard);
    if stream.is_some() {
        reused = true;
    }
    if stream.is_none() {
        stream = Some(connect(
            &dns,
            &facts,
            url,
            proxy,
            dns_timeout,
            connect_timeout,
            tls_timeout,
            total_deadline,
            http2,
            http11,
            h2c,
            tls,
        )?);
    }
    let mut stream = stream.unwrap();
    if reused {
        if let HttpStream::H2(connection) = &stream {
            let probe = {
                let mut connection = connection
                    .lock()
                    .map_err(|_| JetHttpBridgeError::Internal)?;
                if connection.active_streams.is_empty() {
                    set_stream_timeouts(
                        &mut connection.io,
                        first_byte_timeout,
                        write_timeout,
                        total_deadline,
                    )?;
                    connection.probe()
                } else {
                    Ok(())
                }
            };
            if probe.is_err() {
                pool.lock()
                    .map_err(|_| JetHttpBridgeError::Internal)?
                    .remove_h2(&h2_key, connection);
                stream = connect(
                    &dns,
                    &facts,
                    url,
                    proxy,
                    dns_timeout,
                    connect_timeout,
                    tls_timeout,
                    total_deadline,
                    http2,
                    http11,
                    h2c,
                    tls,
                )?;
                reused = false;
            }
        }
    }
    let key = if stream.is_h2() { h2_key } else { h1_key };
    if stream.is_h2() {
        let HttpStream::H2(connection) = stream else {
            unreachable!()
        };
        if !reused {
            pool.lock()
                .map_err(|_| JetHttpBridgeError::Internal)?
                .put(key.clone(), HttpStream::H2(connection.clone()));
        }
        let (stream_id, remote_address) = {
            let mut connection = connection
                .lock()
                .map_err(|_| JetHttpBridgeError::Internal)?;
            set_stream_timeouts(
                &mut connection.io,
                first_byte_timeout,
                write_timeout,
                total_deadline,
            )?;
            let stream_id = connection.start_request(
                method,
                url,
                headers,
                body_len,
                has_body,
                body_read,
                tee,
                decompress,
                &facts,
            )?;
            let remote_address = connection
                .io
                .peer_addr()
                .map(|address| address.to_string())
                .unwrap_or_default();
            (stream_id, remote_address)
        };
        let first_started = Instant::now();
        let first_deadline = Instant::now()
            .checked_add(first_byte_timeout)
            .map(|deadline| total_deadline.map_or(deadline, |total| total.min(deadline)))
            .ok_or(JetHttpBridgeError::Timeout)?;
        let (status, response_headers, end_stream) = loop {
            if Instant::now() >= first_deadline {
                return Err(JetHttpBridgeError::Timeout);
            }
            let polled = {
                let mut connection = connection
                    .lock()
                    .map_err(|_| JetHttpBridgeError::Internal)?;
                let slice = first_deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(10));
                set_stream_timeouts(&mut connection.io, slice, write_timeout, total_deadline)?;
                connection.poll_response_headers(stream_id)?
            };
            if let Some(response) = polled {
                break response;
            }
            std::thread::yield_now();
        };
        set_timing(&facts, 4, elapsed_ms(first_started));
        return read_h2_response(
            connection,
            stream_id,
            status,
            response_headers,
            end_stream,
            key,
            pool,
            decompress,
            facts,
            request_started,
            read_timeout,
            total_deadline,
            permit,
            reused,
            remote_address,
        );
    }
    let request = encode_request(method, url, proxy, headers, body_len, chunked, decompress)?;
    set_stream_timeouts(
        &mut stream,
        first_byte_timeout,
        write_timeout,
        total_deadline,
    )?;
    let write_started = Instant::now();
    if let Err((error, wrote_any)) = write_request(&mut stream, &request) {
        // D-HTTP-CLIENT2: reconnect only on stale-pool Io before any request
        // bytes — never Timeout / post-write first-byte failures.
        if reused
            && !wrote_any
            && matches!(error, JetHttpBridgeError::Io)
            && connection_retry_allowed(retry_policy, method)
        {
            let mut fresh = connect(
                &dns,
                &facts,
                url,
                proxy,
                dns_timeout,
                connect_timeout,
                tls_timeout,
                total_deadline,
                false,
                true,
                false,
                tls,
            )?;
            set_stream_timeouts(
                &mut fresh,
                first_byte_timeout,
                write_timeout,
                total_deadline,
            )?;
            write_request(&mut fresh, &request).map_err(|(error, _)| error)?;
            stream = fresh;
            reused = false;
        } else {
            return Err(error);
        }
    }
    if has_body {
        if let Err((error, _)) = write_upload_body(&mut stream, chunked, &mut *body_read, tee) {
            return Err(error);
        }
    }
    set_timing(&facts, 3, elapsed_ms(write_started));
    let remote_address = stream
        .peer_addr()
        .map(|address| address.to_string())
        .unwrap_or_default();
    let mut response = read_response(
        stream,
        key,
        pool,
        method == "HEAD",
        decompress,
        facts,
        request_started,
        read_timeout,
        total_deadline,
        permit,
    )?;
    response.reused_connection = reused;
    response.remote_address = remote_address;
    Ok(response)
}

fn http_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

fn connection_retry_allowed(policy: RetryPolicy, method: &str) -> bool {
    match policy {
        RetryPolicy::None => false,
        RetryPolicy::Safe => matches!(method, "GET" | "HEAD" | "OPTIONS" | "TRACE"),
        RetryPolicy::Idempotent => {
            matches!(method, "GET" | "HEAD" | "PUT" | "DELETE" | "OPTIONS" | "TRACE")
        }
    }
}

/// Methods whose body can survive a 307/308 follow. 301/302/303 rewrite POST to
/// GET and drop the body; GET/HEAD/OPTIONS/TRACE never need an upload tee.
fn redirect_may_replay_body(method: &str) -> bool {
    !matches!(method, "GET" | "HEAD" | "OPTIONS" | "TRACE")
}

fn connect(
    dns: &Arc<Mutex<DnsCache>>,
    facts: &Arc<Mutex<ResponseFacts>>,
    url: &ParsedUrl,
    proxy: Option<&ParsedUrl>,
    dns_timeout: Duration,
    connect_timeout: Duration,
    tls_timeout: Duration,
    deadline: Option<Instant>,
    http2: bool,
    http11: bool,
    h2c: bool,
    tls: TlsSettings<'_>,
) -> Result<HttpStream, JetHttpBridgeError> {
    let destination = proxy.unwrap_or(url);
    let timeout = remaining_timeout(connect_timeout, deadline)?;
    let mut tcp = connect_plain(
        dns,
        facts,
        destination,
        remaining_timeout(dns_timeout, deadline)?,
        timeout,
    )?;
    if url.scheme == "https" && proxy.is_some() {
        tcp.set_read_timeout(Some(remaining_timeout(tls_timeout, deadline)?))
            .map_err(|_| JetHttpBridgeError::Proxy)?;
        tcp.set_write_timeout(Some(remaining_timeout(tls_timeout, deadline)?))
            .map_err(|_| JetHttpBridgeError::Proxy)?;
        let authority = format!("{}:{}", url.host, url.port);
        write!(tcp, "CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\nProxy-Connection: keep-alive\r\n\r\n")
            .map_err(|_| JetHttpBridgeError::Proxy)?;
        let (head, _, _) = read_head_bytes(&mut tcp).map_err(|_| JetHttpBridgeError::Proxy)?;
        let status = parse_status_and_headers(&head)
            .map_err(|_| JetHttpBridgeError::Proxy)?
            .0;
        if status != 200 {
            return Err(JetHttpBridgeError::Proxy);
        }
    }
    if url.scheme == "https" {
        let started = Instant::now();
        let stream = tls_stream(
            tcp,
            &url.host,
            remaining_timeout(tls_timeout, deadline)?,
            http2,
            http11,
            tls,
        );
        set_timing(facts, 2, elapsed_ms(started));
        stream
    } else if h2c && http2 && proxy.is_none() {
        Ok(HttpStream::H2(Arc::new(Mutex::new(H2Connection::new(
            HttpStream::Plain(tcp),
        )?))))
    } else if http11 {
        Ok(HttpStream::Plain(tcp))
    } else {
        Err(JetHttpBridgeError::Protocol)
    }
}

fn set_stream_timeouts(
    stream: &mut HttpStream,
    read: Duration,
    write: Duration,
    deadline: Option<Instant>,
) -> Result<(), JetHttpBridgeError> {
    let read = remaining_timeout(read, deadline)?;
    let write = remaining_timeout(write, deadline)?;
    let tcp = match stream {
        HttpStream::Plain(stream) => stream,
        HttpStream::Tls(stream) => stream.get_mut(),
        HttpStream::H2(connection) => {
            let mut connection = connection
                .lock()
                .map_err(|_| JetHttpBridgeError::Internal)?;
            return set_stream_timeouts(&mut connection.io, read, write, deadline);
        }
    };
    tcp.set_read_timeout(Some(read))
        .map_err(|_| JetHttpBridgeError::Io)?;
    tcp.set_write_timeout(Some(write))
        .map_err(|_| JetHttpBridgeError::Io)?;
    Ok(())
}

fn encode_request(
    method: &str,
    url: &ParsedUrl,
    proxy: Option<&ParsedUrl>,
    headers: &[(String, String)],
    body_len: Option<usize>,
    chunked: bool,
    decompress: bool,
) -> Result<Vec<u8>, JetHttpBridgeError> {
    let target = if proxy.is_some() && url.scheme == "http" {
        format!("{}://{}{}", url.scheme, url.authority, url.target)
    } else {
        url.target.clone()
    };
    let mut out = format!("{method} {target} HTTP/1.1\r\nHost: {}\r\n", url.authority).into_bytes();
    let mut has_length = false;
    let mut has_connection = false;
    let mut has_accept_encoding = false;
    let mut has_transfer = false;
    for (name, value) in headers {
        has_length |= name.eq_ignore_ascii_case("content-length");
        has_connection |= name.eq_ignore_ascii_case("connection");
        has_accept_encoding |= name.eq_ignore_ascii_case("accept-encoding");
        has_transfer |= name.eq_ignore_ascii_case("transfer-encoding");
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(b": ");
        out.extend_from_slice(value.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    if decompress && !has_accept_encoding {
        out.extend_from_slice(b"Accept-Encoding: gzip\r\n");
    }
    if !has_connection {
        out.extend_from_slice(b"Connection: keep-alive\r\n");
    }
    if chunked {
        if !has_transfer {
            out.extend_from_slice(b"Transfer-Encoding: chunked\r\n");
        }
    } else if let Some(length) = body_len {
        if !has_length {
            out.extend_from_slice(format!("Content-Length: {length}\r\n").as_bytes());
        }
    }
    out.extend_from_slice(b"\r\n");
    Ok(out)
}

fn tee_upload_chunk(tee: &mut Option<Vec<u8>>, chunk: &[u8]) -> Result<(), JetHttpBridgeError> {
    let Some(buffer) = tee.as_mut() else {
        return Ok(());
    };
    let new_len = buffer
        .len()
        .checked_add(chunk.len())
        .ok_or(JetHttpBridgeError::ResourceUnavailable)?;
    if new_len > HTTP_UPLOAD_REPLAY_CAP {
        return Err(JetHttpBridgeError::ResourceUnavailable);
    }
    buffer.extend_from_slice(chunk);
    Ok(())
}

fn write_upload_body(
    stream: &mut HttpStream,
    chunked: bool,
    mut body_read: impl FnMut() -> Result<Option<Vec<u8>>, JetHttpBridgeError>,
    tee: &mut Option<Vec<u8>>,
) -> Result<(), (JetHttpBridgeError, bool)> {
    let mut wrote_any = false;
    loop {
        let chunk = body_read().map_err(|error| (error, wrote_any))?;
        let Some(chunk) = chunk else {
            break;
        };
        if chunk.is_empty() {
            continue;
        }
        tee_upload_chunk(tee, &chunk).map_err(|error| (error, wrote_any))?;
        if chunked {
            let framing = format!("{:x}\r\n", chunk.len());
            write_request(stream, framing.as_bytes()).map_err(|(error, wrote)| (error, wrote_any || wrote))?;
            wrote_any = true;
            write_request(stream, &chunk).map_err(|(error, _)| (error, true))?;
            write_request(stream, b"\r\n").map_err(|(error, _)| (error, true))?;
        } else {
            write_request(stream, &chunk).map_err(|(error, wrote)| (error, wrote_any || wrote))?;
            wrote_any = true;
        }
    }
    if chunked {
        write_request(stream, b"0\r\n\r\n").map_err(|(error, wrote)| (error, wrote_any || wrote))?;
    }
    Ok(())
}

fn slice_body_reader<'a>(
    bytes: &'a [u8],
    offset: &'a mut usize,
) -> impl FnMut() -> Result<Option<Vec<u8>>, JetHttpBridgeError> + 'a {
    move || {
        if *offset >= bytes.len() {
            return Ok(None);
        }
        let end = (*offset + HTTP_UPLOAD_CHUNK).min(bytes.len());
        let chunk = bytes[*offset..end].to_vec();
        *offset = end;
        Ok(Some(chunk))
    }
}

fn write_request(stream: &mut HttpStream, bytes: &[u8]) -> Result<(), (JetHttpBridgeError, bool)> {
    let mut written = 0;
    while written < bytes.len() {
        match stream.write(&bytes[written..]) {
            Ok(0) => return Err((JetHttpBridgeError::Io, written != 0)),
            Ok(count) => written += count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                return Err((JetHttpBridgeError::Timeout, written != 0));
            }
            Err(_) => return Err((JetHttpBridgeError::Io, written != 0)),
        }
    }
    stream
        .flush()
        .map_err(|_| (JetHttpBridgeError::Io, written != 0))
}

fn read_head_bytes(stream: &mut impl Read) -> Result<(Vec<u8>, Vec<u8>, i64), JetHttpBridgeError> {
    read_head_bytes_after(stream, Vec::new())
}

fn read_head_bytes_after(
    stream: &mut impl Read,
    mut bytes: Vec<u8>,
) -> Result<(Vec<u8>, Vec<u8>, i64), JetHttpBridgeError> {
    const MAX_HEAD: usize = 64 * 1024;
    let mut chunk = [0u8; 4096];
    let started = Instant::now();
    let mut first_byte_ms = None;
    loop {
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            let body = bytes.split_off(end + 4);
            bytes.truncate(end + 4);
            return Ok((
                bytes,
                body,
                first_byte_ms.unwrap_or_else(|| elapsed_ms(started)),
            ));
        }
        if bytes.len() >= MAX_HEAD {
            return Err(JetHttpBridgeError::InvalidHeader);
        }
        let read = stream.read(&mut chunk).map_err(|error| {
            if matches!(
                error.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            ) {
                JetHttpBridgeError::Timeout
            } else {
                JetHttpBridgeError::Io
            }
        })?;
        if read == 0 {
            return Err(JetHttpBridgeError::Io);
        }
        if first_byte_ms.is_none() {
            first_byte_ms = Some(elapsed_ms(started));
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
}

fn parse_status_and_headers(
    head: &[u8],
) -> Result<(i64, String, Vec<(String, String)>), JetHttpBridgeError> {
    let text = std::str::from_utf8(head).map_err(|_| JetHttpBridgeError::InvalidHeader)?;
    let mut lines = text
        .strip_suffix("\r\n\r\n")
        .ok_or(JetHttpBridgeError::InvalidFraming)?
        .split("\r\n");
    let status_line = lines.next().ok_or(JetHttpBridgeError::InvalidFraming)?;
    let mut status_parts = status_line.splitn(3, ' ');
    let version = status_parts
        .next()
        .ok_or(JetHttpBridgeError::InvalidFraming)?;
    if !version.starts_with("HTTP/") {
        return Err(JetHttpBridgeError::InvalidFraming);
    }
    if !matches!(version, "HTTP/1.0" | "HTTP/1.1") {
        return Err(JetHttpBridgeError::Protocol);
    }
    let status = status_parts
        .next()
        .ok_or(JetHttpBridgeError::InvalidFraming)?
        .parse::<i64>()
        .map_err(|_| JetHttpBridgeError::InvalidFraming)?;
    if !(100..=599).contains(&status) || status_parts.next().is_none() {
        return Err(JetHttpBridgeError::InvalidFraming);
    }
    let mut headers = Vec::new();
    for line in lines {
        if headers.len() == 100 {
            return Err(JetHttpBridgeError::InvalidHeader);
        }
        if line.starts_with([' ', '\t']) {
            return Err(JetHttpBridgeError::InvalidHeader);
        }
        let (name, value) = line
            .split_once(':')
            .ok_or(JetHttpBridgeError::InvalidHeader)?;
        if name.is_empty()
            || !name.bytes().all(http_token_byte)
            || value.bytes().any(|byte| matches!(byte, 0 | b'\r' | b'\n'))
        {
            return Err(JetHttpBridgeError::InvalidHeader);
        }
        headers.push((
            name.to_ascii_lowercase(),
            value.trim_matches([' ', '\t']).to_string(),
        ));
    }
    Ok((status, version.to_string(), headers))
}

fn header_first<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn content_length(headers: &[(String, String)]) -> Result<Option<usize>, JetHttpBridgeError> {
    let mut found = None;
    for value in headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .flat_map(|(_, value)| value.split(','))
    {
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(JetHttpBridgeError::InvalidFraming);
        }
        let length = value
            .parse::<usize>()
            .map_err(|_| JetHttpBridgeError::InvalidFraming)?;
        if found.is_some_and(|prior| prior != length) {
            return Err(JetHttpBridgeError::InvalidFraming);
        }
        found = Some(length);
    }
    Ok(found)
}

fn decoded_gzip(
    headers: &[(String, String)],
    decompress: bool,
) -> Result<bool, JetHttpBridgeError> {
    let Some(encoding) = header_first(headers, "content-encoding") else {
        return Ok(false);
    };
    if !decompress || encoding.eq_ignore_ascii_case("identity") {
        return Ok(false);
    }
    if encoding.eq_ignore_ascii_case("gzip") {
        Ok(true)
    } else {
        Err(JetHttpBridgeError::UnsupportedEncoding)
    }
}

fn read_h2_response(
    connection: Arc<Mutex<H2Connection>>,
    stream_id: u32,
    status: i64,
    headers: Vec<(String, String)>,
    end_stream: bool,
    _key: PoolKey,
    _pool: Arc<Mutex<ClientPool>>,
    decompress: bool,
    facts: Arc<Mutex<ResponseFacts>>,
    request_started: Instant,
    read_timeout: Duration,
    total_deadline: Option<Instant>,
    permit: OriginPermit,
    reused_connection: bool,
    remote_address: String,
) -> Result<NativeResponse, JetHttpBridgeError> {
    if let Ok(mut response_facts) = facts.lock() {
        response_facts.raw_content_encoding =
            header_first(&headers, "content-encoding").map(str::to_string);
    }
    let length = content_length(&headers)?;
    let reader = H2BodyReader {
        connection: Some(connection),
        stream_id,
        pending: Vec::new(),
        cursor: 0,
        end_after_pending: end_stream,
        finished: false,
        expected: length,
        received: 0,
        facts: facts.clone(),
        request_started,
        read_timeout,
        total_deadline,
        permit: Some(permit),
    };
    let encoded_gzip = decoded_gzip(&headers, decompress)?;
    let reader: Box<dyn Read + Send> = if encoded_gzip {
        Box::new(GzipReader::new(reader))
    } else {
        Box::new(reader)
    };
    let body_handle = register_body(reader);
    let public_length = if encoded_gzip {
        None
    } else {
        length.and_then(|length| i64::try_from(length).ok())
    };
    let public_headers = if encoded_gzip {
        headers
            .into_iter()
            .filter(|(name, _)| !matches!(name.as_str(), "content-encoding" | "content-length"))
            .collect()
    } else {
        headers
    };
    Ok(NativeResponse {
        status,
        headers: public_headers,
        body_handle,
        body_length: public_length,
        reused_connection,
        remote_address,
        protocol: "HTTP/2".to_string(),
        facts,
    })
}

struct H2BodyReader {
    connection: Option<Arc<Mutex<H2Connection>>>,
    stream_id: u32,
    pending: Vec<u8>,
    cursor: usize,
    end_after_pending: bool,
    finished: bool,
    expected: Option<usize>,
    received: usize,
    facts: Arc<Mutex<ResponseFacts>>,
    request_started: Instant,
    read_timeout: Duration,
    total_deadline: Option<Instant>,
    permit: Option<OriginPermit>,
}

impl H2BodyReader {
    fn release_stream(&self) {
        if let Some(session) = &self.connection {
            if let Ok(mut connection) = session.try_lock() {
                connection.active_streams.remove(&self.stream_id);
            }
        }
    }

    fn finish(&mut self) -> std::io::Result<()> {
        if self.finished {
            return Ok(());
        }
        if self
            .expected
            .is_some_and(|expected| expected != self.received)
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "HTTP/2 content-length mismatch",
            ));
        }
        self.finished = true;
        self.release_stream();
        self.permit.take();
        if let Ok(mut facts) = self.facts.lock() {
            facts.timings_ms[6] = elapsed_ms(self.request_started);
        }
        self.connection.take();
        Ok(())
    }

    fn refill(&mut self) -> std::io::Result<()> {
        let session = self
            .connection
            .as_ref()
            .expect("HTTP/2 response connection")
            .clone();
        let mut connection = session
            .lock()
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "HTTP/2 session lock"))?;
        set_stream_timeouts(
            &mut connection.io,
            self.read_timeout,
            self.read_timeout,
            self.total_deadline,
        )
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::TimedOut, format!("{error:?}")))?;
        loop {
            let frame = match connection
                .streams
                .get_mut(&self.stream_id)
                .and_then(VecDeque::pop_front)
                .or_else(|| connection.pending.pop_front())
            {
                Some(frame) => frame,
                None => h2_read_frame(&mut connection.io).map_err(h2_reader_error)?,
            };
            if connection.control(&frame).map_err(h2_reader_error)? {
                continue;
            }
            if frame.stream != self.stream_id {
                connection
                    .streams
                    .entry(frame.stream)
                    .or_default()
                    .push_back(frame);
                continue;
            }
            match frame.kind {
                0 => {
                    let mut payload = frame.payload;
                    if frame.flags & 8 != 0 {
                        let padding = usize::from(*payload.first().ok_or_else(|| {
                            std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "missing HTTP/2 padding",
                            )
                        })?);
                        payload.remove(0);
                        payload.truncate(payload.len().checked_sub(padding).ok_or_else(|| {
                            std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "invalid HTTP/2 padding",
                            )
                        })?);
                    }
                    self.received = self.received.checked_add(payload.len()).ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "HTTP/2 body too large",
                        )
                    })?;
                    if self
                        .expected
                        .is_some_and(|expected| self.received > expected)
                    {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "HTTP/2 content-length mismatch",
                        ));
                    }
                    self.pending = payload;
                    self.cursor = 0;
                    self.end_after_pending = frame.flags & 1 != 0;
                    if self.pending.is_empty() && self.end_after_pending {
                        connection.active_streams.remove(&self.stream_id);
                        self.finish()?;
                    }
                    return Ok(());
                }
                1 => {
                    let (block, end) =
                        h2_header_block(&mut connection.io, frame).map_err(h2_reader_error)?;
                    let trailers = connection.decoder.decode(&block).map_err(h2_reader_error)?;
                    if trailers.iter().any(|(name, _)| name.starts_with(':')) {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "pseudo-header in HTTP/2 trailers",
                        ));
                    }
                    if end {
                        connection.active_streams.remove(&self.stream_id);
                        self.finish()?;
                        return Ok(());
                    }
                }
                3 => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::ConnectionReset,
                        "HTTP/2 stream reset",
                    ))
                }
                _ => {}
            }
        }
    }
}

impl Drop for H2BodyReader {
    fn drop(&mut self) {
        self.release_stream();
    }
}

fn h2_reader_error(error: JetHttpBridgeError) -> std::io::Error {
    std::io::Error::new(
        if error == JetHttpBridgeError::Timeout {
            std::io::ErrorKind::TimedOut
        } else {
            std::io::ErrorKind::InvalidData
        },
        format!("{error:?}"),
    )
}

impl Read for H2BodyReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if out.is_empty() || self.finished {
            return Ok(0);
        }
        let started = Instant::now();
        if self.cursor == self.pending.len() {
            if self.end_after_pending {
                self.finish()?;
                return Ok(0);
            }
            self.refill()?;
            if self.finished {
                return Ok(0);
            }
        }
        let count = out.len().min(self.pending.len() - self.cursor);
        out[..count].copy_from_slice(&self.pending[self.cursor..self.cursor + count]);
        self.cursor += count;
        if self.cursor == self.pending.len() {
            if !self.end_after_pending {
                if let Some(session) = self.connection.as_ref() {
                    let mut connection = session.lock().map_err(|_| {
                        std::io::Error::new(std::io::ErrorKind::Other, "HTTP/2 session lock")
                    })?;
                    let amount = u32::try_from(self.pending.len())
                        .unwrap_or(u32::MAX)
                        .to_be_bytes();
                    h2_write_frame(&mut connection.io, 8, 0, 0, &amount)
                        .map_err(h2_reader_error)?;
                    h2_write_frame(&mut connection.io, 8, 0, self.stream_id, &amount)
                        .map_err(h2_reader_error)?;
                }
            }
            self.pending.clear();
            self.cursor = 0;
            if self.end_after_pending {
                self.finish()?;
            }
        }
        if let Ok(mut facts) = self.facts.lock() {
            facts.timings_ms[5] = facts.timings_ms[5].max(elapsed_ms(started));
        }
        Ok(count)
    }
}

fn read_response(
    mut stream: HttpStream,
    key: PoolKey,
    pool: Arc<Mutex<ClientPool>>,
    head_request: bool,
    decompress: bool,
    facts: Arc<Mutex<ResponseFacts>>,
    request_started: Instant,
    read_timeout: Duration,
    total_deadline: Option<Instant>,
    permit: OriginPermit,
) -> Result<NativeResponse, JetHttpBridgeError> {
    let (status, version, headers, initial) = {
        let mut pending = Vec::new();
        let mut first_byte_ms = None;
        let mut interim_count = 0usize;
        loop {
            let (head, after, elapsed) = read_head_bytes_after(&mut stream, pending)?;
            first_byte_ms.get_or_insert(elapsed);
            let (status, version, headers) = parse_status_and_headers(&head)?;
            if status == 101 {
                return Err(JetHttpBridgeError::Protocol);
            }
            if (100..=199).contains(&status) {
                interim_count += 1;
                if interim_count > 16 {
                    return Err(JetHttpBridgeError::InvalidFraming);
                }
                pending = after;
                continue;
            }
            set_timing(&facts, 4, first_byte_ms.unwrap_or(elapsed));
            break (status, version, headers, after);
        }
    };
    set_stream_timeouts(&mut stream, read_timeout, read_timeout, total_deadline)?;
    if let Ok(mut response_facts) = facts.lock() {
        response_facts.raw_content_encoding =
            header_first(&headers, "content-encoding").map(str::to_string);
    }
    let transfer = headers
        .iter()
        .filter(|(name, _)| name == "transfer-encoding")
        .flat_map(|(_, value)| value.split(','))
        .map(|value| value.trim().to_ascii_lowercase())
        .collect::<Vec<_>>();
    if transfer.len() > 1 || transfer.first().is_some_and(|value| value != "chunked") {
        return Err(JetHttpBridgeError::InvalidFraming);
    }
    let length = content_length(&headers)?;
    if !transfer.is_empty() && length.is_some() {
        return Err(JetHttpBridgeError::InvalidFraming);
    }
    let bodyless = head_request || matches!(status, 100..=199 | 204 | 304);
    let close = version == "HTTP/1.0"
        || headers.iter().any(|(name, value)| {
            name == "connection"
                && value
                    .split(',')
                    .any(|item| item.trim().eq_ignore_ascii_case("close"))
        });
    let framing = if bodyless {
        BodyFraming::Length(0)
    } else if !transfer.is_empty() {
        BodyFraming::Chunked
    } else if let Some(length) = length {
        BodyFraming::Length(length)
    } else {
        BodyFraming::Close
    };
    let reusable = !close && !matches!(framing, BodyFraming::Close);
    let reader = ResponseBodyReader {
        stream: Some(stream),
        key,
        pool,
        initial,
        cursor: 0,
        framing,
        reusable,
        chunk_remaining: 0,
        finished: false,
        facts: facts.clone(),
        request_started,
        permit: Some(permit),
        read_timeout,
        total_deadline,
    };
    let encoded_gzip = decoded_gzip(&headers, decompress)?;
    let reader: Box<dyn Read + Send> = if encoded_gzip {
        Box::new(GzipReader::new(reader))
    } else {
        Box::new(reader)
    };
    let body_handle = register_body(reader);
    let public_length = if encoded_gzip {
        None
    } else {
        length.and_then(|length| i64::try_from(length).ok())
    };
    let public_headers = if encoded_gzip {
        headers
            .into_iter()
            .filter(|(name, _)| !matches!(name.as_str(), "content-encoding" | "content-length"))
            .collect()
    } else {
        headers
    };
    Ok(NativeResponse {
        status,
        headers: public_headers,
        body_handle,
        body_length: public_length,
        reused_connection: false,
        remote_address: String::new(),
        protocol: "HTTP/1.1".to_string(),
        facts,
    })
}

#[derive(Clone, Copy)]
enum BodyFraming {
    Length(usize),
    Chunked,
    Close,
}

struct ResponseBodyReader {
    stream: Option<HttpStream>,
    key: PoolKey,
    pool: Arc<Mutex<ClientPool>>,
    initial: Vec<u8>,
    cursor: usize,
    framing: BodyFraming,
    reusable: bool,
    chunk_remaining: usize,
    finished: bool,
    facts: Arc<Mutex<ResponseFacts>>,
    request_started: Instant,
    permit: Option<OriginPermit>,
    read_timeout: Duration,
    total_deadline: Option<Instant>,
}

impl ResponseBodyReader {
    fn finish(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;
        self.permit.take();
        if let Ok(mut facts) = self.facts.lock() {
            facts.timings_ms[6] = elapsed_ms(self.request_started);
        }
        if self.reusable {
            if let Some(stream) = self.stream.take() {
                let _ = self
                    .pool
                    .lock()
                    .map(|mut pool| pool.put(self.key.clone(), stream));
            }
        }
    }

    fn read_raw(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.cursor < self.initial.len() {
            let count = out.len().min(self.initial.len() - self.cursor);
            out[..count].copy_from_slice(&self.initial[self.cursor..self.cursor + count]);
            self.cursor += count;
            if self.cursor == self.initial.len() {
                self.initial.clear();
                self.cursor = 0;
            }
            return Ok(count);
        }
        let stream = self.stream.as_mut().expect("response stream");
        set_stream_timeouts(
            stream,
            self.read_timeout,
            self.read_timeout,
            self.total_deadline,
        )
        .map_err(|error| {
            std::io::Error::new(
                if error == JetHttpBridgeError::Timeout {
                    std::io::ErrorKind::TimedOut
                } else {
                    std::io::ErrorKind::Other
                },
                format!("{error:?}"),
            )
        })?;
        stream.read(out)
    }

    fn read_exact_raw(&mut self, out: &mut [u8]) -> std::io::Result<()> {
        let mut filled = 0;
        while filled < out.len() {
            let read = self.read_raw(&mut out[filled..])?;
            if read == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "truncated HTTP body",
                ));
            }
            filled += read;
        }
        Ok(())
    }

    fn chunk_size(&mut self) -> std::io::Result<usize> {
        let mut line = Vec::new();
        loop {
            let mut byte = [0];
            self.read_exact_raw(&mut byte)?;
            line.push(byte[0]);
            if line.ends_with(b"\r\n") {
                break;
            }
            if line.len() > 1024 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "oversized chunk line",
                ));
            }
        }
        line.truncate(line.len() - 2);
        let digits = line.split(|byte| *byte == b';').next().unwrap_or(&[]);
        let text = std::str::from_utf8(digits).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid chunk size")
        })?;
        usize::from_str_radix(text, 16)
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid chunk size"))
    }
}

impl Read for ResponseBodyReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if out.is_empty() || self.finished {
            return Ok(0);
        }
        let started = Instant::now();
        let result = match self.framing {
            BodyFraming::Length(0) => {
                self.finish();
                Ok(0)
            }
            BodyFraming::Length(remaining) => {
                let wanted = remaining.min(out.len());
                let read = self.read_raw(&mut out[..wanted])?;
                if read == 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "truncated HTTP body",
                    ));
                }
                self.framing = BodyFraming::Length(remaining - read);
                if remaining == read {
                    self.finish();
                }
                Ok(read)
            }
            BodyFraming::Close => {
                let read = self.read_raw(out)?;
                if read == 0 {
                    self.finished = true;
                    self.stream.take();
                }
                Ok(read)
            }
            BodyFraming::Chunked => {
                if self.chunk_remaining == 0 {
                    self.chunk_remaining = self.chunk_size()?;
                    if self.chunk_remaining == 0 {
                        let mut end = [0; 2];
                        self.read_exact_raw(&mut end)?;
                        if end != *b"\r\n" {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "HTTP trailers unsupported",
                            ));
                        }
                        self.finish();
                        return Ok(0);
                    }
                }
                let wanted = self.chunk_remaining.min(out.len());
                let read = self.read_raw(&mut out[..wanted])?;
                if read == 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "truncated chunk",
                    ));
                }
                self.chunk_remaining -= read;
                if self.chunk_remaining == 0 {
                    let mut end = [0; 2];
                    self.read_exact_raw(&mut end)?;
                    if end != *b"\r\n" {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "invalid chunk ending",
                        ));
                    }
                }
                Ok(read)
            }
        };
        if let Ok(mut facts) = self.facts.lock() {
            facts.timings_ms[5] = facts.timings_ms[5].max(elapsed_ms(started));
        }
        result
    }
}

const GZIP_COMPRESSED_LIMIT: usize = 64 * 1024 * 1024;
const GZIP_DECODED_LIMIT: usize = 8 * 1024 * 1024;

fn invalid_gzip() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid gzip body")
}

struct GzipBits<R> {
    inner: R,
    current: u8,
    remaining: u8,
    compressed: usize,
}

impl<R: Read> GzipBits<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            current: 0,
            remaining: 0,
            compressed: 0,
        }
    }

    fn next_byte(&mut self) -> std::io::Result<u8> {
        if self.compressed == GZIP_COMPRESSED_LIMIT {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "gzip body too large",
            ));
        }
        let mut byte = [0];
        self.inner.read_exact(&mut byte)?;
        self.compressed += 1;
        Ok(byte[0])
    }

    fn bits(&mut self, count: usize) -> std::io::Result<u32> {
        let mut value = 0;
        for offset in 0..count {
            if self.remaining == 0 {
                self.current = self.next_byte()?;
                self.remaining = 8;
            }
            value |= u32::from(self.current & 1) << offset;
            self.current >>= 1;
            self.remaining -= 1;
        }
        Ok(value)
    }

    fn align(&mut self) {
        self.remaining = 0;
    }

    fn aligned_byte(&mut self) -> std::io::Result<u8> {
        self.align();
        self.next_byte()
    }

    fn ensure_eof(&mut self) -> std::io::Result<()> {
        self.align();
        let mut byte = [0];
        match self.inner.read(&mut byte)? {
            0 => Ok(()),
            _ => Err(invalid_gzip()),
        }
    }
}

#[derive(Clone)]
struct Huffman {
    symbols: Vec<(u32, u8, u16)>,
    max: u8,
}

fn reverse_bits(mut code: u32, length: u8) -> u32 {
    let mut reversed = 0;
    for _ in 0..length {
        reversed = reversed << 1 | code & 1;
        code >>= 1;
    }
    reversed
}

impl Huffman {
    fn from_lengths(lengths: &[u8]) -> Result<Self, ()> {
        let max = lengths.iter().copied().max().unwrap_or(0);
        if max == 0 || max > 15 {
            return Err(());
        }
        let mut counts = [0u32; 16];
        for &length in lengths {
            if length > 15 {
                return Err(());
            }
            if length != 0 {
                counts[length as usize] += 1;
            }
        }
        let mut left = 1i32;
        for bits in 1..=15 {
            left = left * 2 - counts[bits] as i32;
            if left < 0 {
                return Err(());
            }
        }
        let mut next = [0u32; 16];
        let mut code = 0;
        for bits in 1..=15 {
            code = (code + counts[bits - 1]) << 1;
            next[bits] = code;
        }
        let mut symbols = Vec::new();
        for (symbol, &length) in lengths.iter().enumerate() {
            if length == 0 {
                continue;
            }
            let canonical = next[length as usize];
            next[length as usize] += 1;
            symbols.push((reverse_bits(canonical, length), length, symbol as u16));
        }
        Ok(Self { symbols, max })
    }

    fn decode<R: Read>(&self, bits: &mut GzipBits<R>) -> std::io::Result<u16> {
        let mut code = 0;
        for length in 1..=self.max {
            code |= bits.bits(1)? << (length - 1);
            if let Some((_, _, symbol)) = self
                .symbols
                .iter()
                .find(|(candidate, len, _)| *len == length && *candidate == code)
            {
                return Ok(*symbol);
            }
        }
        Err(invalid_gzip())
    }
}

fn fixed_trees() -> Result<(Huffman, Huffman), ()> {
    let mut literals = vec![0u8; 288];
    literals[..144].fill(8);
    literals[144..256].fill(9);
    literals[256..280].fill(7);
    literals[280..].fill(8);
    Ok((
        Huffman::from_lengths(&literals)?,
        Huffman::from_lengths(&[5; 32])?,
    ))
}

fn dynamic_trees<R: Read>(bits: &mut GzipBits<R>) -> std::io::Result<(Huffman, Huffman)> {
    let literal_count = bits.bits(5)? as usize + 257;
    let distance_count = bits.bits(5)? as usize + 1;
    let code_count = bits.bits(4)? as usize + 4;
    let order = [
        16usize, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];
    let mut code_lengths = [0u8; 19];
    for index in 0..code_count {
        code_lengths[order[index]] = bits.bits(3)? as u8;
    }
    let code_tree = Huffman::from_lengths(&code_lengths).map_err(|_| invalid_gzip())?;
    let mut lengths = Vec::with_capacity(literal_count + distance_count);
    while lengths.len() < literal_count + distance_count {
        match code_tree.decode(bits)? {
            value @ 0..=15 => lengths.push(value as u8),
            16 => {
                let prior = *lengths.last().ok_or_else(invalid_gzip)?;
                let repeat = bits.bits(2)? as usize + 3;
                lengths.extend(std::iter::repeat(prior).take(repeat));
            }
            17 => {
                let repeat = bits.bits(3)? as usize + 3;
                lengths.extend(std::iter::repeat(0).take(repeat));
            }
            18 => {
                let repeat = bits.bits(7)? as usize + 11;
                lengths.extend(std::iter::repeat(0).take(repeat));
            }
            _ => return Err(invalid_gzip()),
        }
        if lengths.len() > literal_count + distance_count {
            return Err(invalid_gzip());
        }
    }
    Ok((
        Huffman::from_lengths(&lengths[..literal_count]).map_err(|_| invalid_gzip())?,
        Huffman::from_lengths(&lengths[literal_count..]).map_err(|_| invalid_gzip())?,
    ))
}

const LENGTH_BASE: [usize; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [usize; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [usize; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [usize; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

enum InflateBlock {
    GzipHeader,
    DeflateHeader,
    Stored(usize),
    Huffman {
        literals: Huffman,
        distances: Huffman,
    },
    Trailer,
    Done,
}

struct GzipReader<R> {
    bits: GzipBits<R>,
    block: InflateBlock,
    final_block: bool,
    window: VecDeque<u8>,
    copy_distance: usize,
    copy_remaining: usize,
    crc: u32,
    decoded: usize,
    failed: bool,
}

impl<R: Read> GzipReader<R> {
    fn new(inner: R) -> Self {
        Self {
            bits: GzipBits::new(inner),
            block: InflateBlock::GzipHeader,
            final_block: false,
            window: VecDeque::with_capacity(32 * 1024),
            copy_distance: 0,
            copy_remaining: 0,
            crc: !0,
            decoded: 0,
            failed: false,
        }
    }

    fn record(&mut self, byte: u8) -> std::io::Result<u8> {
        if self.decoded == GZIP_DECODED_LIMIT {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "gzip decoded body too large",
            ));
        }
        self.crc ^= u32::from(byte);
        for _ in 0..8 {
            self.crc = if self.crc & 1 != 0 {
                self.crc >> 1 ^ 0xedb8_8320
            } else {
                self.crc >> 1
            };
        }
        if self.window.len() == 32 * 1024 {
            self.window.pop_front();
        }
        self.window.push_back(byte);
        self.decoded += 1;
        Ok(byte)
    }

    fn read_gzip_header(&mut self) -> std::io::Result<()> {
        let mut header = [0; 10];
        for byte in &mut header {
            *byte = self.bits.aligned_byte()?;
        }
        if header[..3] != [0x1f, 0x8b, 8] || header[3] & 0xe0 != 0 {
            return Err(invalid_gzip());
        }
        let flags = header[3];
        if flags & 4 != 0 {
            let length = usize::from(u16::from_le_bytes([
                self.bits.aligned_byte()?,
                self.bits.aligned_byte()?,
            ]));
            for _ in 0..length {
                self.bits.aligned_byte()?;
            }
        }
        for flag in [8, 16] {
            if flags & flag != 0 {
                while self.bits.aligned_byte()? != 0 {}
            }
        }
        if flags & 2 != 0 {
            self.bits.aligned_byte()?;
            self.bits.aligned_byte()?;
        }
        Ok(())
    }

    fn trailer(&mut self) -> std::io::Result<()> {
        self.bits.align();
        let mut crc = [0; 4];
        let mut size = [0; 4];
        for byte in &mut crc {
            *byte = self.bits.aligned_byte()?;
        }
        for byte in &mut size {
            *byte = self.bits.aligned_byte()?;
        }
        if u32::from_le_bytes(crc) != !self.crc || u32::from_le_bytes(size) != self.decoded as u32 {
            return Err(invalid_gzip());
        }
        self.bits.ensure_eof()
    }

    fn next_decoded(&mut self) -> std::io::Result<Option<u8>> {
        loop {
            if self.copy_remaining != 0 {
                if self.copy_distance == 0 || self.copy_distance > self.window.len() {
                    return Err(invalid_gzip());
                }
                let byte = self.window[self.window.len() - self.copy_distance];
                self.copy_remaining -= 1;
                return self.record(byte).map(Some);
            }
            match &mut self.block {
                InflateBlock::GzipHeader => {
                    self.read_gzip_header()?;
                    self.block = InflateBlock::DeflateHeader;
                }
                InflateBlock::DeflateHeader => {
                    self.final_block = self.bits.bits(1)? != 0;
                    self.block = match self.bits.bits(2)? {
                        0 => {
                            self.bits.align();
                            let length = self.bits.bits(16)? as u16;
                            if self.bits.bits(16)? as u16 != !length {
                                return Err(invalid_gzip());
                            }
                            InflateBlock::Stored(usize::from(length))
                        }
                        1 => {
                            let (literals, distances) =
                                fixed_trees().map_err(|_| invalid_gzip())?;
                            InflateBlock::Huffman {
                                literals,
                                distances,
                            }
                        }
                        2 => {
                            let (literals, distances) = dynamic_trees(&mut self.bits)?;
                            InflateBlock::Huffman {
                                literals,
                                distances,
                            }
                        }
                        _ => return Err(invalid_gzip()),
                    };
                }
                InflateBlock::Stored(remaining) => {
                    if *remaining == 0 {
                        self.block = if self.final_block {
                            InflateBlock::Trailer
                        } else {
                            InflateBlock::DeflateHeader
                        };
                        continue;
                    }
                    *remaining -= 1;
                    let byte = self.bits.bits(8)? as u8;
                    return self.record(byte).map(Some);
                }
                InflateBlock::Huffman {
                    literals,
                    distances,
                } => match literals.decode(&mut self.bits)? {
                    literal @ 0..=255 => return self.record(literal as u8).map(Some),
                    256 => {
                        self.block = if self.final_block {
                            InflateBlock::Trailer
                        } else {
                            InflateBlock::DeflateHeader
                        };
                    }
                    symbol @ 257..=285 => {
                        let index = symbol as usize - 257;
                        self.copy_remaining =
                            LENGTH_BASE[index] + self.bits.bits(LENGTH_EXTRA[index])? as usize;
                        let distance_symbol = distances.decode(&mut self.bits)? as usize;
                        if distance_symbol >= DIST_BASE.len() {
                            return Err(invalid_gzip());
                        }
                        self.copy_distance = DIST_BASE[distance_symbol]
                            + self.bits.bits(DIST_EXTRA[distance_symbol])? as usize;
                    }
                    _ => return Err(invalid_gzip()),
                },
                InflateBlock::Trailer => {
                    self.trailer()?;
                    self.block = InflateBlock::Done;
                    return Ok(None);
                }
                InflateBlock::Done => return Ok(None),
            }
        }
    }
}

impl<R: Read> Read for GzipReader<R> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.failed {
            return Err(invalid_gzip());
        }
        if out.is_empty() {
            return Ok(0);
        }
        let mut written = 0;
        while written < out.len() {
            match self.next_decoded() {
                Ok(Some(byte)) => {
                    out[written] = byte;
                    written += 1;
                }
                Ok(None) => break,
                Err(error) => {
                    self.failed = true;
                    if written == 0 {
                        return Err(error);
                    }
                    break;
                }
            }
        }
        Ok(written)
    }
}

const HPACK_STATIC: [(&str, &str); 61] = [
    (":authority", ""),
    (":method", "GET"),
    (":method", "POST"),
    (":path", "/"),
    (":path", "/index.html"),
    (":scheme", "http"),
    (":scheme", "https"),
    (":status", "200"),
    (":status", "204"),
    (":status", "206"),
    (":status", "304"),
    (":status", "400"),
    (":status", "404"),
    (":status", "500"),
    ("accept-charset", ""),
    ("accept-encoding", "gzip, deflate"),
    ("accept-language", ""),
    ("accept-ranges", ""),
    ("accept", ""),
    ("access-control-allow-origin", ""),
    ("age", ""),
    ("allow", ""),
    ("authorization", ""),
    ("cache-control", ""),
    ("content-disposition", ""),
    ("content-encoding", ""),
    ("content-language", ""),
    ("content-length", ""),
    ("content-location", ""),
    ("content-range", ""),
    ("content-type", ""),
    ("cookie", ""),
    ("date", ""),
    ("etag", ""),
    ("expect", ""),
    ("expires", ""),
    ("from", ""),
    ("host", ""),
    ("if-match", ""),
    ("if-modified-since", ""),
    ("if-none-match", ""),
    ("if-range", ""),
    ("if-unmodified-since", ""),
    ("last-modified", ""),
    ("link", ""),
    ("location", ""),
    ("max-forwards", ""),
    ("proxy-authenticate", ""),
    ("proxy-authorization", ""),
    ("range", ""),
    ("referer", ""),
    ("refresh", ""),
    ("retry-after", ""),
    ("server", ""),
    ("set-cookie", ""),
    ("strict-transport-security", ""),
    ("transfer-encoding", ""),
    ("user-agent", ""),
    ("vary", ""),
    ("via", ""),
    ("www-authenticate", ""),
];

fn hpack_integer(out: &mut Vec<u8>, value: usize, prefix: u8, first: u8) {
    let mask = (1usize << prefix) - 1;
    if value < mask {
        out.push(first | value as u8);
        return;
    }
    out.push(first | mask as u8);
    let mut rest = value - mask;
    while rest >= 128 {
        out.push((rest as u8 & 0x7f) | 0x80);
        rest >>= 7;
    }
    out.push(rest as u8);
}

fn hpack_string(out: &mut Vec<u8>, value: &str) {
    hpack_integer(out, value.len(), 7, 0);
    out.extend_from_slice(value.as_bytes());
}

fn hpack_literal(out: &mut Vec<u8>, name: &str, value: &str, sensitive: bool) {
    let name_index = HPACK_STATIC
        .iter()
        .position(|(candidate, _)| *candidate == name)
        .map(|index| index + 1)
        .unwrap_or(0);
    hpack_integer(out, name_index, 4, if sensitive { 0x10 } else { 0 });
    if name_index == 0 {
        hpack_string(out, name);
    }
    hpack_string(out, value);
}

fn hpack_request(
    method: &str,
    url: &ParsedUrl,
    headers: &[(String, String)],
    body_len: Option<usize>,
    decompress: bool,
) -> Result<Vec<u8>, JetHttpBridgeError> {
    let mut out = Vec::new();
    match method {
        "GET" => out.push(0x82),
        "POST" => out.push(0x83),
        value => hpack_literal(&mut out, ":method", value, false),
    }
    if url.target == "/" {
        out.push(0x84);
    } else {
        hpack_literal(&mut out, ":path", &url.target, false);
    }
    out.push(if url.scheme == "https" { 0x87 } else { 0x86 });
    hpack_literal(&mut out, ":authority", &url.authority, false);
    let mut has_length = false;
    let mut has_accept_encoding = false;
    for (name, value) in headers {
        let name = name.to_ascii_lowercase();
        if matches!(
            name.as_str(),
            "connection" | "proxy-connection" | "keep-alive" | "upgrade" | "transfer-encoding"
        ) {
            continue;
        }
        if name == "te" && !value.eq_ignore_ascii_case("trailers") {
            return Err(JetHttpBridgeError::InvalidHeader);
        }
        has_length |= name == "content-length";
        has_accept_encoding |= name == "accept-encoding";
        hpack_literal(
            &mut out,
            &name,
            value,
            matches!(
                name.as_str(),
                "authorization" | "proxy-authorization" | "cookie" | "set-cookie"
            ),
        );
    }
    if decompress && !has_accept_encoding {
        hpack_literal(&mut out, "accept-encoding", "gzip", false);
    }
    if let Some(length) = body_len {
        if !has_length {
            hpack_literal(&mut out, "content-length", &length.to_string(), false);
        }
    }
    Ok(out)
}

#[derive(Default)]
struct HpackDecoder {
    dynamic: VecDeque<(String, String)>,
    size: usize,
    max: usize,
}

impl HpackDecoder {
    fn new() -> Self {
        Self {
            dynamic: VecDeque::new(),
            size: 0,
            max: 4096,
        }
    }

    fn entry(&self, index: usize) -> Result<(String, String), JetHttpBridgeError> {
        if index == 0 {
            return Err(JetHttpBridgeError::Protocol);
        }
        if index <= HPACK_STATIC.len() {
            let (name, value) = HPACK_STATIC[index - 1];
            return Ok((name.to_string(), value.to_string()));
        }
        self.dynamic
            .get(index - HPACK_STATIC.len() - 1)
            .cloned()
            .ok_or(JetHttpBridgeError::Protocol)
    }

    fn insert(&mut self, name: String, value: String) {
        let size = name.len().saturating_add(value.len()).saturating_add(32);
        if size > self.max {
            self.dynamic.clear();
            self.size = 0;
            return;
        }
        while self.size.saturating_add(size) > self.max {
            if let Some((name, value)) = self.dynamic.pop_back() {
                self.size -= name.len() + value.len() + 32;
            } else {
                break;
            }
        }
        self.dynamic.push_front((name, value));
        self.size += size;
    }

    fn decode(&mut self, bytes: &[u8]) -> Result<Vec<(String, String)>, JetHttpBridgeError> {
        let mut cursor = 0usize;
        let mut out = Vec::new();
        let mut saw_field = false;
        while cursor < bytes.len() {
            let first = bytes[cursor];
            if first & 0x80 != 0 {
                let index = hpack_read_integer(bytes, &mut cursor, 7)?;
                out.push(self.entry(index)?);
                saw_field = true;
            } else if first & 0x40 != 0 {
                let index = hpack_read_integer(bytes, &mut cursor, 6)?;
                let name = if index == 0 {
                    hpack_read_string(bytes, &mut cursor)?
                } else {
                    self.entry(index)?.0
                };
                let value = hpack_read_string(bytes, &mut cursor)?;
                self.insert(name.clone(), value.clone());
                out.push((name, value));
                saw_field = true;
            } else if first & 0x20 != 0 {
                if saw_field {
                    return Err(JetHttpBridgeError::Protocol);
                }
                let max = hpack_read_integer(bytes, &mut cursor, 5)?;
                if max > 4096 {
                    return Err(JetHttpBridgeError::Protocol);
                }
                self.max = max;
                while self.size > self.max {
                    let (name, value) = self
                        .dynamic
                        .pop_back()
                        .ok_or(JetHttpBridgeError::Protocol)?;
                    self.size -= name.len() + value.len() + 32;
                }
            } else {
                let index = hpack_read_integer(bytes, &mut cursor, 4)?;
                let name = if index == 0 {
                    hpack_read_string(bytes, &mut cursor)?
                } else {
                    self.entry(index)?.0
                };
                let value = hpack_read_string(bytes, &mut cursor)?;
                out.push((name, value));
                saw_field = true;
            }
            if out.len() > 100
                || out
                    .iter()
                    .map(|(name, value)| name.len() + value.len() + 32)
                    .sum::<usize>()
                    > 64 * 1024
            {
                return Err(JetHttpBridgeError::InvalidHeader);
            }
        }
        Ok(out)
    }
}

fn hpack_read_integer(
    bytes: &[u8],
    cursor: &mut usize,
    prefix: u8,
) -> Result<usize, JetHttpBridgeError> {
    let first = *bytes.get(*cursor).ok_or(JetHttpBridgeError::Protocol)?;
    *cursor += 1;
    let mask = (1usize << prefix) - 1;
    let mut value = usize::from(first) & mask;
    if value < mask {
        return Ok(value);
    }
    let mut shift = 0usize;
    loop {
        let byte = *bytes.get(*cursor).ok_or(JetHttpBridgeError::Protocol)?;
        *cursor += 1;
        if shift > usize::BITS as usize - 7 {
            return Err(JetHttpBridgeError::Protocol);
        }
        value = value
            .checked_add(usize::from(byte & 0x7f) << shift)
            .ok_or(JetHttpBridgeError::Protocol)?;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
        shift += 7;
    }
}

fn hpack_read_string(bytes: &[u8], cursor: &mut usize) -> Result<String, JetHttpBridgeError> {
    let huffman = bytes.get(*cursor).is_some_and(|byte| byte & 0x80 != 0);
    let length = hpack_read_integer(bytes, cursor, 7)?;
    if length > 64 * 1024 {
        return Err(JetHttpBridgeError::InvalidHeader);
    }
    let end = cursor
        .checked_add(length)
        .filter(|end| *end <= bytes.len())
        .ok_or(JetHttpBridgeError::Protocol)?;
    let value = if huffman {
        hpack_huffman_decode(&bytes[*cursor..end])?
    } else {
        bytes[*cursor..end].to_vec()
    };
    *cursor = end;
    String::from_utf8(value).map_err(|_| JetHttpBridgeError::InvalidHeader)
}

fn hpack_huffman_decode(bytes: &[u8]) -> Result<Vec<u8>, JetHttpBridgeError> {
    const CODES: [u32; 257] = [
        0x1ff8, 0x7fffd8, 0xfffffe2, 0xfffffe3, 0xfffffe4, 0xfffffe5, 0xfffffe6, 0xfffffe7,
        0xfffffe8, 0xffffea, 0x3ffffffc, 0xfffffe9, 0xfffffea, 0x3ffffffd, 0xfffffeb, 0xfffffec,
        0xfffffed, 0xfffffee, 0xfffffef, 0xffffff0, 0xffffff1, 0xffffff2, 0x3ffffffe, 0xffffff3,
        0xffffff4, 0xffffff5, 0xffffff6, 0xffffff7, 0xffffff8, 0xffffff9, 0xffffffa, 0xffffffb,
        0x14, 0x3f8, 0x3f9, 0xffa, 0x1ff9, 0x15, 0xf8, 0x7fa, 0x3fa, 0x3fb, 0xf9, 0x7fb, 0xfa,
        0x16, 0x17, 0x18, 0x0, 0x1, 0x2, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x5c, 0xfb,
        0x7ffc, 0x20, 0xffb, 0x3fc, 0x1ffa, 0x21, 0x5d, 0x5e, 0x5f, 0x60, 0x61, 0x62, 0x63, 0x64,
        0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x6b, 0x6c, 0x6d, 0x6e, 0x6f, 0x70, 0x71, 0x72, 0xfc,
        0x73, 0xfd, 0x1ffb, 0x7fff0, 0x1ffc, 0x3ffc, 0x22, 0x7ffd, 0x3, 0x23, 0x4, 0x24, 0x5, 0x25,
        0x26, 0x27, 0x6, 0x74, 0x75, 0x28, 0x29, 0x2a, 0x7, 0x2b, 0x76, 0x2c, 0x8, 0x9, 0x2d, 0x77,
        0x78, 0x79, 0x7a, 0x7b, 0x7ffe, 0x7fc, 0x3ffd, 0x1ffd, 0xffffffc, 0xfffe6, 0x3fffd2,
        0xfffe7, 0xfffe8, 0x3fffd3, 0x3fffd4, 0x3fffd5, 0x7fffd9, 0x3fffd6, 0x7fffda, 0x7fffdb,
        0x7fffdc, 0x7fffdd, 0x7fffde, 0xffffeb, 0x7fffdf, 0xffffec, 0xffffed, 0x3fffd7, 0x7fffe0,
        0xffffee, 0x7fffe1, 0x7fffe2, 0x7fffe3, 0x7fffe4, 0x1fffdc, 0x3fffd8, 0x7fffe5, 0x3fffd9,
        0x7fffe6, 0x7fffe7, 0xffffef, 0x3fffda, 0x1fffdd, 0xfffe9, 0x3fffdb, 0x3fffdc, 0x7fffe8,
        0x7fffe9, 0x1fffde, 0x7fffea, 0x3fffdd, 0x3fffde, 0xfffff0, 0x1fffdf, 0x3fffdf, 0x7fffeb,
        0x7fffec, 0x1fffe0, 0x1fffe1, 0x3fffe0, 0x1fffe2, 0x7fffed, 0x3fffe1, 0x7fffee, 0x7fffef,
        0xfffea, 0x3fffe2, 0x3fffe3, 0x3fffe4, 0x7ffff0, 0x3fffe5, 0x3fffe6, 0x7ffff1, 0x3ffffe0,
        0x3ffffe1, 0xfffeb, 0x7fff1, 0x3fffe7, 0x7ffff2, 0x3fffe8, 0x1ffffec, 0x3ffffe2, 0x3ffffe3,
        0x3ffffe4, 0x7ffffde, 0x7ffffdf, 0x3ffffe5, 0xfffff1, 0x1ffffed, 0x7fff2, 0x1fffe3,
        0x3ffffe6, 0x7ffffe0, 0x7ffffe1, 0x3ffffe7, 0x7ffffe2, 0xfffff2, 0x1fffe4, 0x1fffe5,
        0x3ffffe8, 0x3ffffe9, 0xffffffd, 0x7ffffe3, 0x7ffffe4, 0x7ffffe5, 0xfffec, 0xfffff3,
        0xfffed, 0x1fffe6, 0x3fffe9, 0x1fffe7, 0x1fffe8, 0x7ffff3, 0x3fffea, 0x3fffeb, 0x1ffffee,
        0x1ffffef, 0xfffff4, 0xfffff5, 0x3ffffea, 0x7ffff4, 0x3ffffeb, 0x7ffffe6, 0x3ffffec,
        0x3ffffed, 0x7ffffe7, 0x7ffffe8, 0x7ffffe9, 0x7ffffea, 0x7ffffeb, 0xffffffe, 0x7ffffec,
        0x7ffffed, 0x7ffffee, 0x7ffffef, 0x7fffff0, 0x3ffffee, 0x3fffffff,
    ];
    const LENGTHS: [u8; 257] = [
        13, 23, 28, 28, 28, 28, 28, 28, 28, 24, 30, 28, 28, 30, 28, 28, 28, 28, 28, 28, 28, 28, 30,
        28, 28, 28, 28, 28, 28, 28, 28, 28, 6, 10, 10, 12, 13, 6, 8, 11, 10, 10, 8, 11, 8, 6, 6, 6,
        5, 5, 5, 6, 6, 6, 6, 6, 6, 6, 7, 8, 15, 6, 12, 10, 13, 6, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7,
        7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 8, 7, 8, 13, 19, 13, 14, 6, 15, 5, 6, 5, 6, 5, 6, 6, 6, 5,
        7, 7, 6, 6, 6, 5, 6, 7, 6, 5, 5, 6, 7, 7, 7, 7, 7, 15, 11, 14, 13, 28, 20, 22, 20, 20, 22,
        22, 22, 23, 22, 23, 23, 23, 23, 23, 24, 23, 24, 24, 22, 23, 24, 23, 23, 23, 23, 21, 22, 23,
        22, 23, 23, 24, 22, 21, 20, 22, 22, 23, 23, 21, 23, 22, 22, 24, 21, 22, 23, 23, 21, 21, 22,
        21, 23, 22, 23, 23, 20, 22, 22, 22, 23, 22, 22, 23, 26, 26, 20, 19, 22, 23, 22, 25, 26, 26,
        26, 27, 27, 26, 24, 25, 19, 21, 26, 27, 27, 26, 27, 24, 21, 21, 26, 26, 28, 27, 27, 27, 20,
        24, 20, 21, 22, 21, 21, 23, 22, 22, 25, 25, 24, 24, 26, 23, 26, 27, 26, 26, 27, 27, 27, 27,
        27, 28, 27, 27, 27, 27, 27, 26, 30,
    ];
    #[derive(Clone, Copy)]
    struct Node {
        next: [i16; 2],
        symbol: i16,
    }
    static TREE: OnceLock<Vec<Node>> = OnceLock::new();
    let tree = TREE.get_or_init(|| {
        let mut nodes = vec![Node {
            next: [-1; 2],
            symbol: -1,
        }];
        for (symbol, (&code, length)) in CODES.iter().zip(LENGTHS).enumerate() {
            let mut node = 0usize;
            for shift in (0..length).rev() {
                let bit = usize::from(((code >> shift) & 1) as u8);
                if nodes[node].next[bit] < 0 {
                    let next = nodes.len();
                    nodes.push(Node {
                        next: [-1; 2],
                        symbol: -1,
                    });
                    nodes[node].next[bit] = next as i16;
                }
                node = nodes[node].next[bit] as usize;
            }
            nodes[node].symbol = symbol as i16;
        }
        nodes
    });
    let mut out = Vec::new();
    let mut node = 0usize;
    let mut length = 0u8;
    let mut tail = 0u8;
    for &byte in bytes {
        for shift in (0..8).rev() {
            let bit = usize::from((byte >> shift) & 1);
            length += 1;
            tail = tail << 1 | ((byte >> shift) & 1);
            let next = tree[node].next[bit];
            if next < 0 {
                return Err(JetHttpBridgeError::Protocol);
            }
            node = next as usize;
            if tree[node].symbol >= 0 {
                let symbol = tree[node].symbol as usize;
                if symbol == 256 {
                    return Err(JetHttpBridgeError::Protocol);
                }
                out.push(symbol as u8);
                node = 0;
                length = 0;
                tail = 0;
                if out.len() > 64 * 1024 {
                    return Err(JetHttpBridgeError::InvalidHeader);
                }
            }
        }
    }
    if length > 7 || length != 0 && tail != (1u8 << length) - 1 {
        return Err(JetHttpBridgeError::Protocol);
    }
    Ok(out)
}

fn coalesce_request_headers(flat: &[String]) -> Vec<(String, String)> {
    let mut headers: Vec<(String, String)> = Vec::new();
    for pair in flat.chunks_exact(2) {
        if let Some((_, value)) = headers
            .iter_mut()
            .find(|(name, _)| name.eq_ignore_ascii_case(&pair[0]))
        {
            value.push_str(if pair[0].eq_ignore_ascii_case("cookie") {
                "; "
            } else {
                ", "
            });
            value.push_str(&pair[1]);
        } else {
            headers.push((pair[0].clone(), pair[1].clone()));
        }
    }
    headers
}

fn encode_component(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn encode_form(fields: &[String]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i + 1 < fields.len() {
        if !out.is_empty() {
            out.push('&');
        }
        out.push_str(&encode_component(&fields[i]));
        out.push('=');
        out.push_str(&encode_component(&fields[i + 1]));
        i += 2;
    }
    out
}

fn multipart_boundary(fields: &[String]) -> String {
    const PREFIX: &str = "jet-http-boundary-";
    const LENGTH: usize = PREFIX.len() + 16;

    let mut used = std::collections::HashSet::new();
    for field in fields {
        for window in field.as_bytes().windows(LENGTH) {
            let Some(suffix) = window.strip_prefix(PREFIX.as_bytes()) else {
                continue;
            };
            let Ok(suffix) = std::str::from_utf8(suffix) else {
                continue;
            };
            if let Ok(suffix) = u64::from_str_radix(suffix, 16) {
                used.insert(suffix);
            }
        }
    }

    let mut suffix = 0u64;
    while used.contains(&suffix) {
        suffix = suffix
            .checked_add(1)
            .expect("in-memory multipart fields cannot contain every u64 boundary suffix");
    }
    format!("{PREFIX}{suffix:016x}")
}

fn encode_multipart(fields: &[String], boundary: &str) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i + 1 < fields.len() {
        out.push_str("--");
        out.push_str(boundary);
        out.push_str("\r\nContent-Disposition: form-data; name=\"");
        out.push_str(
            &fields[i]
                .replace('"', "%22")
                .replace('\r', "%0D")
                .replace('\n', "%0A"),
        );
        out.push_str("\"\r\n\r\n");
        out.push_str(&fields[i + 1]);
        out.push_str("\r\n");
        i += 2;
    }
    out.push_str("--");
    out.push_str(boundary);
    out.push_str("--\r\n");
    out
}
