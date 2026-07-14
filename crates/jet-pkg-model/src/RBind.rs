//! Persistent supervised R binder (D-FFI-R1=A).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindResult {
    pub source: String,
    pub bound: Vec<String>,
    pub archive: PathBuf,
    pub provenance: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindError {
    Source(String),
    ToolMissing(&'static str),
    ToolFailed(&'static str, String),
    Io(String),
}
impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Source(v) | Self::Io(v) => f.write_str(v),
            Self::ToolMissing(v) => write!(f, "the provisioned `{v}` tool was not found"),
            Self::ToolFailed(t, v) => write!(f, "`{t}` rejected the R binding input: {v}"),
        }
    }
}

pub fn bind(path: &Path, source: &str, lib: &str, cache: &Path) -> Result<BindResult, BindError> {
    require_supported_host(cfg!(unix))?;
    if !ident(lib) {
        return Err(BindError::Source(format!(
            "`{lib}` is not a valid Jet library name"
        )));
    }
    let rscript = tool_path("Rscript").ok_or(BindError::ToolMissing("Rscript"))?;
    let script = std::fs::canonicalize(path)
        .map_err(|e| BindError::Io(format!("could not resolve the R script: {e}")))?;
    std::fs::create_dir_all(cache)
        .map_err(|e| BindError::Io(format!("could not create R binding cache: {e}")))?;
    let build = cache.join(format!(".r-build-{lib}"));
    let _ = std::fs::remove_dir_all(&build);
    std::fs::create_dir_all(&build)
        .map_err(|e| BindError::Io(format!("could not create R build directory: {e}")))?;
    let discoverer = build.join("jet_discover.R");
    std::fs::write(&discoverer, DISCOVERER)
        .map_err(|e| BindError::Io(format!("could not write R binding inspector: {e}")))?;
    let metadata = run_capture(
        Command::new(&rscript)
            .args(["--vanilla"])
            .arg(&discoverer)
            .arg(&script),
        "Rscript",
    )?;
    let functions = parse_function_names(&metadata)?;
    let worker = cache.join(format!("{lib}_worker.R"));
    let worker_source = render_worker(&functions);
    std::fs::write(&worker, &worker_source)
        .map_err(|e| BindError::Io(format!("could not write R worker: {e}")))?;
    let worker = std::fs::canonicalize(&worker)
        .map_err(|e| BindError::Io(format!("could not resolve the R worker: {e}")))?;
    let abi = format!("jet_r_{lib}");
    let mut wrappers = String::new();
    for name in &functions {
        wrappers.push_str(&format!("const char* {abi}_invoke_{name}(int64_t h,const char*input,int64_t deadline){{return invoke(h,\"{name}\",input,deadline);}}\n"));
        wrappers.push_str(&format!("const char* {abi}_invoke_{name}_table(int64_t h,const char*input,int64_t deadline){{return invoke(h,\"__jet_table__{name}\",input,deadline);}}\n"));
        wrappers.push_str(&format!("const char* {abi}_invoke_{name}_plot(int64_t h,const char*input,int64_t deadline){{return invoke(h,\"__jet_plot__{name}\",input,deadline);}}\n"));
    }
    let bridge = crate::PowerShellBind::render_supervisor_c_with_temp(
        &abi,
        &rscript,
        &worker,
        &script,
        &wrappers,
        "\"--vanilla\",worker_path,script_path",
        Some("/tmp/jet-r-plot-"),
    );
    let c = build.join(format!("{abi}.c"));
    let object = build.join(format!("{abi}.o"));
    std::fs::write(&c, bridge)
        .map_err(|e| BindError::Io(format!("could not write R process bridge: {e}")))?;
    run(
        Command::new("cc")
            .args(["-std=c11", "-D_POSIX_C_SOURCE=200809L", "-fPIC", "-c"])
            .arg(&c)
            .arg("-o")
            .arg(&object),
        "cc",
    )?;
    let archive = cache.join(format!("lib{abi}.a"));
    let _ = std::fs::remove_file(&archive);
    run(
        Command::new("ar").arg("rcs").arg(&archive).arg(&object),
        "ar",
    )?;
    let mut identity = b"jet-r-bind-v1\0".to_vec();
    identity.extend_from_slice(source.as_bytes());
    identity.push(0);
    identity.extend_from_slice(script.to_string_lossy().as_bytes());
    identity.push(0);
    identity.extend_from_slice(rscript.to_string_lossy().as_bytes());
    identity.push(0);
    identity.extend_from_slice(worker_source.as_bytes());
    let result=BindResult{source:render_jet(lib,&functions),bound:functions,archive,provenance:format!("schema=jet-r-bind-v1\nsha256={}\nrscript={}\nscript={}\nworker={}\nworkers_per_session=1\nmax_sessions=32\ntransport=jsonlite\n",crate::SHA256::sha256_hex(&identity),rscript.display(),script.display(),worker.display())};
    let _ = std::fs::remove_dir_all(&build);
    Ok(result)
}

const DISCOVERER: &str = r#"args <- commandArgs(trailingOnly = TRUE)
expressions <- parse(file = args[[1]], keep.source = FALSE)
for (expression in expressions) {
  if (!is.call(expression) || !(as.character(expression[[1]]) %in% c("<-", "="))) next
  if (!is.symbol(expression[[2]]) || !is.call(expression[[3]]) || !identical(expression[[3]][[1]], as.name("function"))) next
  name <- as.character(expression[[2]])
  parameters <- expression[[3]][[2]]
  valid <- length(parameters) == 1 && names(parameters)[[1]] != "..." && identical(deparse(parameters[[1]]), "")
  if (!valid) { cat("E\tARG\t", name, "\n", sep = ""); quit(status = 0) }
  cat("N\t", name, "\n", sep = "")
}
"#;

// Generated into each worker so plot output is checked where it crosses the
// process boundary. This is a structural XML tokenizer/canonicalizer: regexes
// are used only for scalar value checks after elements and attributes parse.
const SVG_RUNTIME: &str = r##"
svg_error <- function() stop("invalid SVG plot")
svg_name_char <- function(ch, first = FALSE) {
  if (length(ch) == 0 || is.na(ch)) return(FALSE)
  code <- utf8ToInt(ch)
  alpha <- (code >= 65 && code <= 90) || (code >= 97 && code <= 122)
  if (first) return(alpha || ch == "_")
  alpha || (code >= 48 && code <= 57) || ch %in% c("_", "-", ".", ":")
}
svg_safe_id <- function(value) {
  chars <- strsplit(value, "", fixed = TRUE)[[1]]
  length(chars) > 0 && svg_name_char(chars[[1]], TRUE) &&
    (length(chars) == 1 || all(vapply(chars[-1], svg_name_char, logical(1))))
}
svg_escape <- function(value, attribute = FALSE) {
  chars <- strsplit(value, "", fixed = TRUE)[[1]]
  paste0(vapply(chars, function(ch) {
    if (ch == "&") return("&amp;")
    if (ch == "<") return("&lt;")
    if (ch == ">") return("&gt;")
    if (attribute && ch == "\"") return("&quot;")
    ch
  }, character(1)), collapse = "")
}
svg_decode <- function(value) {
  chars <- strsplit(value, "", fixed = TRUE)[[1]]
  out <- character(0); at <- 1L
  while (at <= length(chars)) {
    if (chars[[at]] != "&") { out <- c(out, chars[[at]]); at <- at + 1L; next }
    end <- at + 1L
    while (end <= length(chars) && chars[[end]] != ";" && end - at <= 12L) end <- end + 1L
    if (end > length(chars) || chars[[end]] != ";") svg_error()
    entity <- paste0(chars[(at + 1L):(end - 1L)], collapse = "")
    decoded <- switch(entity, amp = "&", lt = "<", gt = ">", quot = "\"", apos = "'", NULL)
    if (is.null(decoded) && startsWith(entity, "#")) {
      hex <- startsWith(entity, "#x") || startsWith(entity, "#X")
      digits <- substring(entity, if (hex) 3L else 2L)
      if (!nzchar(digits) || grepl(if (hex) "[^0-9A-Fa-f]" else "[^0-9]", digits)) svg_error()
      point <- suppressWarnings(strtoi(digits, base = if (hex) 16L else 10L))
      if (is.na(point) || point < 1L || point > 0x10ffffL || point %in% 0xd800L:0xdfffL ||
          point %in% c(0x7fL, 0xfffeL, 0xffffL) || (point < 0x20L && !(point %in% c(9L, 10L, 13L)))) svg_error()
      decoded <- intToUtf8(point)
    }
    if (is.null(decoded)) svg_error()
    out <- c(out, decoded); at <- end + 1L
  }
  text <- paste0(out, collapse = "")
  points <- utf8ToInt(text)
  if (any(points < 0x20L & !(points %in% c(9L, 10L, 13L)))) svg_error()
  text
}
svg_local_url <- function(value) {
  startsWith(value, "url(#") && endsWith(value, ")") &&
    svg_safe_id(substring(value, 6L, nchar(value) - 1L))
}
svg_safe_scalar <- function(value, allow_local_url = FALSE) {
  lower <- tolower(value)
  denied <- c("javascript:", "vbscript:", "data:", "file:", "http:", "https:", "//", "@import", "expression(", "behavior:", "-moz-binding")
  if (any(vapply(denied, function(item) grepl(item, lower, fixed = TRUE), logical(1)))) svg_error()
  if (grepl("url(", lower, fixed = TRUE) && !(allow_local_url && svg_local_url(value))) svg_error()
  value
}
svg_safe_style <- function(value) {
  properties <- c("fill", "fill-opacity", "fill-rule", "stroke", "stroke-width", "stroke-linecap",
    "stroke-linejoin", "stroke-miterlimit", "stroke-dasharray", "stroke-dashoffset", "stroke-opacity",
    "opacity", "font-family", "font-size", "font-style", "font-weight", "text-anchor", "clip-rule")
  declarations <- strsplit(value, ";", fixed = TRUE)[[1]]
  mapped <- list()
  for (declaration in declarations) {
    declaration <- trimws(declaration)
    if (!nzchar(declaration)) next
    colon <- regexpr(":", declaration, fixed = TRUE)[[1]]
    if (colon < 2L) svg_error()
    name <- tolower(trimws(substring(declaration, 1L, colon - 1L)))
    item <- trimws(substring(declaration, colon + 1L))
    if (!(name %in% properties) || !is.null(mapped[[name]])) svg_error()
    mapped[[name]] <- svg_safe_scalar(item, FALSE)
  }
  if (length(mapped) == 0) return("")
  names <- sort(names(mapped), method = "radix")
  paste0(vapply(names, function(name) paste0(name, ":", mapped[[name]], ";"), character(1)), collapse = "")
}
svg_safe_attribute <- function(name, value) {
  allowed <- c("xmlns", "xmlns:xlink", "width", "height", "viewBox", "version", "x", "y", "x1", "y1",
    "x2", "y2", "cx", "cy", "r", "rx", "ry", "d", "points", "transform", "fill", "fill-opacity",
    "fill-rule", "stroke", "stroke-width", "stroke-linecap", "stroke-linejoin", "stroke-miterlimit",
    "stroke-dasharray", "stroke-dashoffset", "stroke-opacity", "opacity", "font-family", "font-size",
    "font-style", "font-weight", "text-anchor", "clip-path", "clip-rule", "id", "class", "offset",
    "stop-color", "stop-opacity", "preserveAspectRatio", "xlink:href", "href", "style")
  if (startsWith(tolower(name), "on") || !(name %in% allowed)) svg_error()
  if (name == "xmlns") {
    if (value != "http://www.w3.org/2000/svg") svg_error()
    return(value)
  }
  if (name == "xmlns:xlink") {
    if (value != "http://www.w3.org/1999/xlink") svg_error()
    return(value)
  }
  if (name %in% c("href", "xlink:href")) {
    if (!startsWith(value, "#") || !svg_safe_id(substring(value, 2L))) svg_error()
    return(value)
  }
  if (name == "style") return(svg_safe_style(value))
  svg_safe_scalar(value, name %in% c("clip-path", "fill", "stroke"))
}
sanitize_svg <- function(value) {
  if (!is.character(value) || length(value) != 1L || is.na(value) || nchar(value, type = "bytes") > 524288L) svg_error()
  chars <- strsplit(value, "", fixed = TRUE)[[1]]; count <- length(chars); index <- 1L
  stack <- character(0); root <- 0L; output <- character(0)
  allowed_elements <- c("svg", "g", "defs", "symbol", "use", "path", "rect", "circle", "ellipse", "line",
    "polyline", "polygon", "text", "tspan", "clipPath", "linearGradient", "radialGradient", "stop",
    "pattern", "mask", "title", "desc")
  peek <- function(offset = 0L) if (index + offset <= count) chars[[index + offset]] else ""
  starts <- function(text) {
    n <- nchar(text)
    index + n - 1L <= count && paste0(chars[index:(index + n - 1L)], collapse = "") == text
  }
  space <- function(ch) ch %in% c(" ", "\t", "\r", "\n")
  skip_space <- function() while (index <= count && space(chars[[index]])) index <<- index + 1L
  read_name <- function() {
    if (!svg_name_char(peek(), TRUE)) svg_error()
    begin <- index; index <<- index + 1L
    while (index <= count && svg_name_char(chars[[index]])) index <<- index + 1L
    paste0(chars[begin:(index - 1L)], collapse = "")
  }
  read_value <- function() {
    quote <- peek(); if (!(quote %in% c("'", "\""))) svg_error()
    index <<- index + 1L; begin <- index
    while (index <= count && chars[[index]] != quote) {
      if (chars[[index]] == "<") svg_error()
      index <<- index + 1L
    }
    if (index > count) svg_error()
    text <- if (index == begin) "" else paste0(chars[begin:(index - 1L)], collapse = "")
    index <<- index + 1L; svg_decode(text)
  }
  skip_space()
  if (starts("<?xml")) {
    begin <- index; index <- index + 5L
    if (!space(peek())) svg_error()
    while (index <= count && !starts("?>")) index <- index + 1L
    if (index > count) svg_error()
    declaration <- paste0(chars[begin:(index + 1L)], collapse = "")
    if (!(declaration %in% c('<?xml version="1.0"?>', '<?xml version="1.0" encoding="UTF-8"?>'))) svg_error()
    index <- index + 2L; skip_space()
  }
  while (index <= count) {
    if (peek() != "<") {
      begin <- index
      while (index <= count && chars[[index]] != "<") index <- index + 1L
      text <- svg_decode(paste0(chars[begin:(index - 1L)], collapse = ""))
      if (length(stack) == 0) { if (nzchar(trimws(text))) svg_error() } else output <- c(output, svg_escape(text))
      next
    }
    if (starts("<!") || starts("<?")) svg_error()
    if (starts("</")) {
      index <- index + 2L; name <- read_name(); skip_space()
      if (peek() != ">" || length(stack) == 0 || tail(stack, 1L) != name) svg_error()
      index <- index + 1L; stack <- head(stack, -1L); output <- c(output, paste0("</", name, ">")); next
    }
    index <- index + 1L; name <- read_name()
    if (!(name %in% allowed_elements)) svg_error()
    if (length(stack) == 0) { root <- root + 1L; if (name != "svg" || root != 1L) svg_error() }
    attributes <- list(); closed <- FALSE
    repeat {
      skip_space()
      if (starts("/>")) { index <- index + 2L; closed <- TRUE; break }
      if (peek() == ">") { index <- index + 1L; break }
      attr <- read_name(); if (!is.null(attributes[[attr]]) || length(attributes) >= 128L) svg_error()
      skip_space(); if (peek() != "=") svg_error(); index <- index + 1L; skip_space()
      attributes[[attr]] <- svg_safe_attribute(attr, read_value())
    }
    names <- if (length(attributes) == 0) character(0) else sort(names(attributes), method = "radix"); rendered <- ""
    if (length(names) > 0) rendered <- paste0(vapply(names, function(attr) paste0(" ", attr, "=\"", svg_escape(attributes[[attr]], TRUE), "\""), character(1)), collapse = "")
    output <- c(output, paste0("<", name, rendered, if (closed) "/>" else ">"))
    if (!closed) { stack <- c(stack, name); if (length(stack) > 128L) svg_error() }
  }
  if (root != 1L || length(stack) != 0L) svg_error()
  result <- paste0(output, collapse = "")
  if (nchar(result, type = "bytes") > 524288L) svg_error()
  result
}
plot_to_svg <- function(fn, input) {
  directory <- Sys.getenv("JET_BIND_TEMP", unset = "")
  if (!nzchar(directory)) svg_error()
  path <- file.path(directory, "plot.svg"); device <- NULL
  on.exit({
    devices <- dev.list()
    if (!is.null(device) && !is.null(devices) && device %in% devices) try(dev.off(device), silent = TRUE)
    unlink(path)
  }, add = TRUE)
  svg(filename = path, width = 7, height = 5, onefile = TRUE)
  device <- dev.cur(); invisible(fn(input))
  devices <- dev.list()
  if (!is.null(devices) && device %in% devices) dev.off(device)
  device <- NULL
  size <- file.info(path)$size
  if (length(size) != 1L || is.na(size) || size < 1L || size > 524288L) svg_error()
  bytes <- readBin(path, "raw", n = size)
  if (length(bytes) != size || any(bytes == as.raw(0))) svg_error()
  unlink(path)
  sanitize_svg(rawToChar(bytes))
}
"##;

fn parse_function_names(bytes: &[u8]) -> Result<Vec<String>, BindError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| BindError::Source("R returned non-UTF-8 function metadata".into()))?;
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("E\tARG\t") {
            return Err(BindError::Source(format!(
                "R function `{name}` must take one required positional argument"
            )));
        }
        let name = line.strip_prefix("N\t").ok_or_else(|| {
            BindError::Source("R parser returned malformed function metadata".into())
        })?;
        if !ident(name) {
            return Err(BindError::Source(format!(
                "R function `{name}` cannot be projected as a Jet identifier"
            )));
        }
        if reserved(name) {
            return Err(BindError::Source(format!(
                "R function `{name}` uses a reserved generated binding name"
            )));
        }
        if out
            .iter()
            .any(|v: &String| {
                v == name || v == &format!("{name}_table") || v == &format!("{name}_plot")
            })
        {
            return Err(BindError::Source(format!(
                "R function `{name}` collides with a generated binding name"
            )));
        }
        if out.iter().any(|v| format!("{v}_table") == name) {
            return Err(BindError::Source(format!(
                "R function `{name}` collides with a generated table binding"
            )));
        }
        if out.iter().any(|v| format!("{v}_plot") == name) {
            return Err(BindError::Source(format!(
                "R function `{name}` collides with a generated plot binding"
            )));
        }
        out.push(name.to_string());
    }
    if out.is_empty() {
        return Err(BindError::Source(
            "no top-level named R functions were found".into(),
        ));
    }
    Ok(out)
}

fn render_worker(functions: &[String]) -> String {
    let allowed = functions
        .iter()
        .map(|v| format!("  {v} = TRUE"))
        .collect::<Vec<_>>()
        .join(",\n");
    format!(
        r#"suppressPackageStartupMessages(library(jsonlite))
input <- file("/dev/stdin", "rb")
output <- file("/dev/stdout", "wb")
script <- new.env(parent = baseenv())
sys.source(commandArgs(trailingOnly = TRUE)[[1]], envir = script)
allowed <- c(
{allowed}
)
{svg_runtime}
read_exact <- function(length) {{
  value <- raw(0)
  while (length(value) < length) {{
    part <- readBin(input, "raw", n = length - length(value))
    if (length(part) == 0) return(NULL)
    value <- c(value, part)
  }}
  value
}}
little <- function(value, width) as.raw(floor(as.numeric(value) / 256^(0:(width - 1))) %% 256)
from_wire <- function(value) {{
  if (is.list(value)) {{
    mapped <- lapply(value, from_wire)
    names(mapped) <- names(value)
    scalar <- vapply(mapped, function(item) is.atomic(item) && length(item) == 1, logical(1))
    kinds <- unique(vapply(mapped, typeof, character(1)))
    if (is.null(names(value)) && length(mapped) > 0 && all(scalar) && length(kinds) == 1) return(unlist(mapped, use.names = FALSE))
    return(mapped)
  }}
  value
}}
table_from_wire <- function(rows) {{
  if (!is.list(rows)) stop("invalid table rows")
  if (length(rows) == 0) return(data.frame())
  columns <- names(rows[[1]])
  valid_row <- function(row) is.list(row) && identical(names(row), columns)
  if (is.null(columns) || anyDuplicated(columns) || !all(vapply(rows, valid_row, logical(1)))) stop("invalid table row shape")
  frames <- lapply(rows, function(row) {{
    cells <- lapply(row, function(cell) {{
      item <- from_wire(cell)
      if (is.null(item)) return(NA)
      if (!is.atomic(item) || length(item) != 1) stop("table cells must be scalar")
      item
    }})
    as.data.frame(cells, stringsAsFactors = FALSE, optional = TRUE)
  }})
  do.call(rbind, frames)
}}
writeBin(c(little(5, 4), charToRaw("READY")), output)
flush(output)
repeat {{
  header <- read_exact(4)
  if (is.null(header)) break
  size <- sum(as.integer(header) * 256^(0:3))
  if (size < 1 || size > 1048576) break
  payload <- read_exact(size)
  if (is.null(payload)) break
  request <- NULL
  response <- tryCatch({{
    request <- fromJSON(rawToChar(payload), simplifyVector = FALSE)
    if (!is.list(request) || is.null(request$op)) stop("invalid request")
    if (identical(request$op, "shutdown")) NULL else {{
      command <- request$command
      table_mode <- startsWith(command, "__jet_table__")
      plot_mode <- startsWith(command, "__jet_plot__")
      if (table_mode) command <- substring(command, 14)
      if (plot_mode) command <- substring(command, 13)
      if (!identical(request$op, "invoke") || is.null(command) || is.na(allowed[[command]])) stop("rejected command")
      fn <- get(command, envir = script, inherits = FALSE)
      input_value <- if (table_mode) table_from_wire(request$input) else from_wire(request$input)
      value <- if (plot_mode) plot_to_svg(fn, input_value) else fn(input_value)
      list(id = request$id, ok = TRUE, value = value)
    }}
  }}, error = function(error) list(id = if (is.list(request) && !is.null(request$id)) request$id else 0, ok = FALSE, code = "CommandFailed", value = NULL))
  if (is.null(response)) break
  encoded <- charToRaw(toJSON(response, auto_unbox = TRUE, null = "null", na = "null", dataframe = "rows", digits = NA))
  if (length(encoded) > 1048576) break
  writeBin(c(little(length(encoded), 4), little(response$id, 8), encoded), output)
  flush(output)
}}
"#
        , svg_runtime = SVG_RUNTIME
    )
}

fn render_jet(lib: &str, functions: &[String]) -> String {
    let abi = format!("jet_r_{lib}");
    let mut out=format!("#Extern module c.{abi} {{\n    fn open() -> Int = \"{abi}_open\"\n    fn take_error() -> Int = \"{abi}_take_error\"\n    fn cancel(handle: Int) = \"{abi}_cancel\"\n    fn close(handle: Int) = \"{abi}_close\"\n");
    for name in functions {
        out.push_str(&format!("    fn {name}(handle: Int, input: String, deadline_ms: Int) -> String = \"{abi}_invoke_{name}\"\n"));
        out.push_str(&format!("    fn {name}_table(handle: Int, input: String, deadline_ms: Int) -> String = \"{abi}_invoke_{name}_table\"\n"));
        out.push_str(&format!("    fn {name}_plot(handle: Int, input: String, deadline_ms: Int) -> String = \"{abi}_invoke_{name}_plot\"\n"));
    }
    out.push_str(&format!("}}\nuse c.{abi} as abi\nuse core.encoding.json as json\nuse core.data as data\n\npub struct Session {{ value: Int }}\npub enum RError {{ NotRunning Timeout Cancelled Protocol CommandFailed Limit }}\n\npub fn open() -> Session ? RError {{\n    handle :: abi.open()\n    if abi.take_error() != 0 {{ return err(RError.NotRunning) }}\n    return ok(Session.{{ value: handle }})\n}}\n\npub fn cancel(session: Session) {{ abi.cancel(session.value) }}\npub fn close(session: ^Session) {{ abi.close(session.value) }}\n\n"));
    for name in functions {
        out.push_str(&format!("pub fn {name}(session: Session, input: DataTree, deadline_ms: Int) -> DataTree ? RError {{\n    raw :: abi.{name}(session.value, json.to_string(input), deadline_ms)\n    code :: abi.take_error()\n    if code == 1 {{ return err(RError.NotRunning) }}\n    if code == 2 {{ return err(RError.Timeout) }}\n    if code == 3 {{ return err(RError.Cancelled) }}\n    if code == 5 {{ return err(RError.Limit) }}\n    if code != 0 {{ return err(RError.Protocol) }}\n    response := json.parse(raw) ?? return err(RError.Protocol)\n    succeeded := (response.field(\"ok\") ?? DataTree.Bool(false)).bool() ?? false\n    if !succeeded {{ return err(RError.CommandFailed) }}\n    return ok(response.field(\"value\") ?? DataTree.Null)\n}}\n\n"));
        out.push_str(&format!("pub fn {name}_table<T: [Encode, Decode]>(session: Session, table: Table<T>, deadline_ms: Int) -> Table<T> ? RError {{\n    raw :: abi.{name}_table(session.value, json.to_string(data.rows(table)), deadline_ms)\n    code :: abi.take_error()\n    if code == 1 {{ return err(RError.NotRunning) }}\n    if code == 2 {{ return err(RError.Timeout) }}\n    if code == 3 {{ return err(RError.Cancelled) }}\n    if code == 5 {{ return err(RError.Limit) }}\n    if code != 0 {{ return err(RError.Protocol) }}\n    response := json.parse(raw) ?? return err(RError.Protocol)\n    succeeded := (response.field(\"ok\") ?? DataTree.Bool(false)).bool() ?? false\n    if !succeeded {{ return err(RError.CommandFailed) }}\n    value := response.field(\"value\") ?? return err(RError.Protocol)\n    rows := json.decode<[T]>(json.to_string(value)) ?? return err(RError.Protocol)\n    return ok(data.table(rows))\n}}\n\n"));
        out.push_str(&format!("pub fn {name}_plot(session: Session, input: DataTree, deadline_ms: Int) -> String ? RError {{\n    raw :: abi.{name}_plot(session.value, json.to_string(input), deadline_ms)\n    code :: abi.take_error()\n    if code == 1 {{ return err(RError.NotRunning) }}\n    if code == 2 {{ return err(RError.Timeout) }}\n    if code == 3 {{ return err(RError.Cancelled) }}\n    if code == 5 {{ return err(RError.Limit) }}\n    if code != 0 {{ return err(RError.Protocol) }}\n    response := json.parse(raw) ?? return err(RError.Protocol)\n    succeeded := (response.field(\"ok\") ?? DataTree.Bool(false)).bool() ?? false\n    if !succeeded {{ return err(RError.CommandFailed) }}\n    value := (response.field(\"value\") ?? DataTree.Null).text() ?? return err(RError.Protocol)\n    return ok(value)\n}}\n\n"));
    }
    out
}

fn tool_path(tool: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|v| v.join(tool))
        .find(|v| v.is_file())
        .and_then(|v| std::fs::canonicalize(v).ok())
}
fn run(command: &mut Command, tool: &'static str) -> Result<(), BindError> {
    run_capture(command, tool).map(|_| ())
}
fn run_capture(command: &mut Command, tool: &'static str) -> Result<Vec<u8>, BindError> {
    const CAP: usize = 64 * 1024;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            BindError::ToolMissing(tool)
        } else {
            BindError::Io(format!("could not start `{tool}`: {e}"))
        }
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| BindError::Io(format!("could not supervise `{tool}` stdout")))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| BindError::Io(format!("could not supervise `{tool}` stderr")))?;
    let out = std::thread::spawn(move || drain(stdout, CAP));
    let err = std::thread::spawn(move || drain(stderr, CAP));
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        match child
            .try_wait()
            .map_err(|e| BindError::Io(format!("could not supervise `{tool}`: {e}")))?
        {
            Some(v) => break v,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = out.join();
                let _ = err.join();
                return Err(BindError::ToolFailed(
                    tool,
                    "the tool exceeded the 60 second limit".into(),
                ));
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    let stdout = out
        .join()
        .map_err(|_| BindError::Io(format!("`{tool}` stdout reader failed")))??;
    let stderr = err
        .join()
        .map_err(|_| BindError::Io(format!("`{tool}` stderr reader failed")))??;
    if status.success() {
        Ok(stdout)
    } else {
        let _ = stderr;
        Err(BindError::ToolFailed(
            tool,
            "the R parser returned a failure status".into(),
        ))
    }
}
fn drain(mut input: impl Read, limit: usize) -> Result<Vec<u8>, BindError> {
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = input
            .read(&mut buf)
            .map_err(|e| BindError::Io(format!("could not read foreign tool output: {e}")))?;
        if n == 0 {
            break;
        }
        let keep = (limit - out.len()).min(n);
        out.extend_from_slice(&buf[..keep]);
    }
    Ok(out)
}
fn ident(v: &str) -> bool {
    let mut chars = v.chars();
    matches!(chars.next(),Some(c)if c.is_ascii_alphabetic()||c=='_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}
fn reserved(v: &str) -> bool {
    matches!(v, "open" | "cancel" | "close" | "Session" | "RError")
        || crate::Syntax::JET_KEYWORD_LIST.contains(&v)
        || crate::Syntax::JET_TYPE_LIST.contains(&v)
}
fn require_supported_host(unix: bool) -> Result<(), BindError> {
    if unix {
        Ok(())
    } else {
        Err(BindError::Source(
            "persistent R bindings require a POSIX host process supervisor".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_metadata_and_rejects_collisions() {
        assert_eq!(
            super::parse_function_names(b"N\ttransform\n").unwrap(),
            vec!["transform"]
        );
        assert!(super::parse_function_names(b"N\topen\n").is_err());
        assert!(super::parse_function_names(b"N\tx\nN\tx_table\n").is_err());
        assert!(super::parse_function_names(b"N\tx\nN\tx_plot\n").is_err());
    }
    #[test]
    fn rejects_bad_parameters() {
        assert!(super::parse_function_names(b"E\tARG\tbad\n")
            .unwrap_err()
            .to_string()
            .contains("one required positional argument"));
    }
    #[test]
    fn non_posix_hosts_fail() {
        assert!(super::require_supported_host(false).is_err());
    }
}
