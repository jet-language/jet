//! GnuCOBOL C-ABI and copybook binder (D-FFI-COBOL1=A).
//!
//! The accepted copybook subset is deliberately closed: fixed text, native
//! binary integers, and packed decimal. Unknown clauses fail binding rather
//! than guessing a wire layout. COMP-3 is surfaced as Jet Decimal metadata;
//! the callable bridge uses scaled minor units because Decimal is not a C ABI
//! scalar.

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindResult {
    pub source: String,
    pub archive: PathBuf,
    pub runtime_dir: PathBuf,
    pub program: String,
    pub layout: RecordLayout,
    pub provenance: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordLayout { pub name: String, pub width: usize, pub fields: Vec<FieldLayout> }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldLayout {
    pub name: String,
    pub offset: usize,
    pub width: usize,
    pub kind: FieldKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldKind {
    FixedText { characters: usize },
    NativeInt { digits: usize, signed: bool },
    PackedDecimal { digits: usize, scale: usize, signed: bool },
}

impl FieldKind {
    pub fn jet_type(&self) -> &'static str {
        match self { Self::FixedText { .. } => "String", Self::NativeInt { .. } => "Int", Self::PackedDecimal { .. } => "Decimal" }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindError {
    Source(String),
    AbiMismatch(String),
    ToolMissing(&'static str),
    ToolFailed { tool: &'static str, detail: String },
    IO(String),
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Source(s) | Self::IO(s) => f.write_str(s),
            Self::AbiMismatch(s) => write!(f, "the generated COBOL C-ABI bridge failed its link proof: {s}"),
            Self::ToolMissing(t) => write!(f, "the provisioned `{t}` tool was not found"),
            Self::ToolFailed { tool, detail } => write!(f, "`{tool}` rejected the COBOL binding: {detail}"),
        }
    }
}

pub fn bind(source_path: &Path, source: &str, copybook_path: &Path, copybook: &str, lib: &str, cache: &Path) -> Result<BindResult, BindError> {
    if !is_ident(lib) { return Err(BindError::Source(format!("`{lib}` is not a valid Jet library name"))); }
    let program = parse_program_id(source)?;
    let layout = parse_copybook(copybook)?;
    let packed = layout.fields.iter().filter(|f| matches!(f.kind, FieldKind::PackedDecimal { .. })).collect::<Vec<_>>();
    if packed.len() != 1 { return Err(BindError::Source("the callable v1 bridge requires exactly one COMP-3 field".into())); }
    let input = layout.fields.iter().find(|f| matches!(f.kind, FieldKind::NativeInt { .. }));
    std::fs::create_dir_all(cache).map_err(|e| BindError::IO(format!("could not create binding cache: {e}")))?;
    let stem = format!("jet_cobol_{lib}");
    let object = cache.join(format!("{stem}_program.o"));
    let bridge_c = cache.join(format!("{stem}_bridge.c"));
    let bridge_o = cache.join(format!("{stem}_bridge.o"));
    let archive = cache.join(format!("lib{stem}.a"));
    let probe_c = cache.join(format!("{stem}_probe.c"));
    let probe = cache.join(format!("{stem}_probe"));
    let copy_dir = copybook_path.parent().unwrap_or_else(|| Path::new("."));
    run(Command::new("cobc").args(["-c", "-I"]).arg(copy_dir).arg(source_path).arg("-o").arg(&object), "cobc")?;
    let config = checked_output(Command::new("cob-config").arg("--cflags"), "cob-config")?;
    let cflags = String::from_utf8_lossy(&config.stdout).split_whitespace().map(str::to_string).collect::<Vec<_>>();
    let libs = checked_output(Command::new("cob-config").arg("--libs"), "cob-config")?;
    let runtime_flags = String::from_utf8_lossy(&libs.stdout).split_whitespace().map(str::to_string).collect::<Vec<_>>();
    let runtime_dir = runtime_dir_from_flags(&runtime_flags)
        .ok_or_else(|| BindError::Source("cob-config did not report an absolute libcob directory".into()))?;
    std::fs::write(&bridge_c, render_bridge(&program, &layout, packed[0], input, lib))
        .map_err(|e| BindError::IO(format!("could not write the COBOL C bridge: {e}")))?;
    let mut cc = Command::new("cc"); cc.arg("-c").arg("-fPIC"); cc.args(&cflags).arg(&bridge_c).arg("-o").arg(&bridge_o);
    run(&mut cc, "cc")?;
    std::fs::write(&probe_c, "int main(void) { return 0; }\n")
        .map_err(|e| BindError::IO(format!("could not write the COBOL link probe: {e}")))?;
    let mut link = Command::new("cc");
    link.arg("-Wl,--no-undefined").arg(&object).arg(&bridge_o).arg(&probe_c).args(&runtime_flags).args(["-pthread", "-ldl", "-lm"]).arg("-o").arg(&probe);
    if let Err(error) = prove_link(&mut link) {
        let _ = std::fs::remove_file(&object);
        let _ = std::fs::remove_file(&bridge_o);
        let _ = std::fs::remove_file(&bridge_c);
        let _ = std::fs::remove_file(&probe_c);
        let _ = std::fs::remove_file(&probe);
        return Err(error);
    }
    run(Command::new("ar").arg("rcs").arg(&archive).arg(&object).arg(&bridge_o), "ar")?;
    let _ = std::fs::remove_file(&object);
    let _ = std::fs::remove_file(&bridge_o);
    let _ = std::fs::remove_file(&bridge_c);
    let _ = std::fs::remove_file(&probe_c);
    let _ = std::fs::remove_file(&probe);
    let generated_source = render_jet(lib, &layout, packed[0], input);
    let provenance = render_provenance(
        source_path,
        source,
        &generated_source,
        copybook_path,
        copybook,
        &program,
        &layout,
        &runtime_dir,
        &archive,
    )?;
    Ok(BindResult { source: generated_source, archive, runtime_dir, program, layout, provenance })
}

pub fn parse_copybook(source: &str) -> Result<RecordLayout, BindError> {
    let mut record = None; let mut fields = Vec::new(); let mut names = HashSet::new(); let mut offset: usize = 0;
    for raw in source.lines() {
        let Some(line) = copybook_line(raw) else { continue };
        let words = line.split_whitespace().collect::<Vec<_>>();
        if words.len() < 2 { return Err(BindError::Source(format!("unsupported copybook declaration `{line}`; use level 01 with level 05 PIC fields"))); }
        if words[0] == "01" {
            if record.is_some() { return Err(BindError::Source("copybook must contain exactly one level 01 record".into())); }
            if words.len() != 2 { return Err(BindError::Source(format!("unsupported level 01 declaration `{line}`; use one record name only"))); }
            let name = jet_name(words[1]);
            if !is_ident(&name) { return Err(BindError::Source(format!("copybook record name `{}` is not a valid Jet identifier", words[1]))); }
            record = Some(name);
            continue;
        }
        if words[0] != "05" || record.is_none() { return Err(BindError::Source(format!("unsupported copybook declaration `{line}`; use level 01 with level 05 PIC fields"))); }
        let name = jet_name(words[1]);
        if !is_ident(&name) || crate::Syntax::is_reserved_generated_name(&name) || !names.insert(name.clone()) {
            return Err(BindError::Source(format!("copybook field name `{}` is not a unique, usable Jet identifier", words[1])));
        }
        if words.get(2).is_none_or(|word| !word.eq_ignore_ascii_case("PIC")) {
            return Err(BindError::Source(format!("copybook field `{}` must use a PIC clause", words[1])));
        }
        let pic = words.get(3).ok_or_else(|| BindError::Source(format!("copybook field `{}` has no PIC shape", words[1])))?.to_ascii_uppercase();
        let clauses = words[4..].iter().map(|w| w.to_ascii_uppercase()).collect::<Vec<_>>();
        let kind = parse_pic(&pic, &clauses)?;
        let width = match kind { FieldKind::FixedText { characters } => characters, FieldKind::NativeInt { digits, .. } => if digits <= 4 { 2 } else if digits <= 9 { 4 } else if digits <= 18 { 8 } else { return Err(BindError::Source("COMP-5 fields may contain at most 18 digits".into())); }, FieldKind::PackedDecimal { digits, .. } => (digits + 2) / 2 };
        let next_offset = offset.checked_add(width).ok_or_else(|| BindError::Source("copybook record width overflows the supported ABI".into()))?;
        fields.push(FieldLayout { name, offset, width, kind }); offset = next_offset;
    }
    let name = record.ok_or_else(|| BindError::Source("copybook has no level 01 record".into()))?;
    if fields.is_empty() { return Err(BindError::Source("copybook record has no level 05 fields".into())); }
    if crate::Syntax::is_reserved_generated_name(&upper_camel(&name)) {
        return Err(BindError::Source(format!("copybook record name `{name}` projects to a reserved Jet type")));
    }
    Ok(RecordLayout { name, width: offset, fields })
}

fn parse_pic(pic: &str, clauses: &[String]) -> Result<FieldKind, BindError> {
    if let Some(n) = pic.strip_prefix("X(").and_then(|v| v.strip_suffix(')')).and_then(|v| v.parse().ok()) {
        if clauses.is_empty() && n > 0 { return Ok(FieldKind::FixedText { characters: n }); }
    }
    let signed = pic.starts_with('S'); let numeric = pic.strip_prefix('S').unwrap_or(pic);
    let (whole, frac) = numeric.split_once('V').map_or((numeric, None), |(a,b)| (a,Some(b)));
    let digits = pic_digits(whole).and_then(|a| pic_digits(frac.unwrap_or("")).map(|b| a + b));
    let scale = frac.and_then(pic_digits).unwrap_or(0);
    let Some(digits) = digits else { return Err(BindError::Source(format!("unsupported PIC shape `{pic}`"))); };
    if clauses.len() == 1 && clauses[0] == "COMP-3" {
        if digits == 0 || digits > 18 { return Err(BindError::Source("COMP-3 fields may contain 1 to 18 digits for the Int minor-unit bridge".into())); }
        return Ok(FieldKind::PackedDecimal { digits, scale, signed });
    }
    if clauses.len() == 1 && clauses[0] == "COMP-5" && scale == 0 {
        if digits == 0 || digits > 18 { return Err(BindError::Source("COMP-5 fields may contain 1 to 18 digits for the Int bridge".into())); }
        return Ok(FieldKind::NativeInt { digits, signed });
    }
    Err(BindError::Source(format!("unsupported PIC/usage `{pic} {}`; use X(n), COMP-5, or COMP-3", clauses.join(" "))))
}

fn copybook_line(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.starts_with('*') || trimmed.starts_with("*>") {
        return None;
    }
    if raw.len() > 6 {
        let indicator = raw.as_bytes()[6];
        if matches!(indicator, b'*' | b'/') {
            return None;
        }
        let fixed = raw.get(6..).unwrap_or_default().trim();
        if matches!(fixed.split_whitespace().next(), Some("01" | "05")) {
            return Some(fixed.split("*>").next().unwrap_or_default().trim().trim_end_matches('.').trim().to_string());
        }
    }
    Some(trimmed.split("*>").next().unwrap_or_default().trim().trim_end_matches('.').trim().to_string())
}

fn runtime_dir_from_flags(flags: &[String]) -> Option<PathBuf> {
    flags.iter().enumerate().find_map(|(index, flag)| {
        flag.strip_prefix("-L").filter(|path| !path.is_empty()).map(PathBuf::from).or_else(|| {
            (flag == "-L")
                .then(|| flags.get(index + 1))
                .flatten()
                .map(|path| PathBuf::from(path.as_str()))
        })
    }).filter(|path| path.is_absolute())
}

fn prove_link(command: &mut Command) -> Result<(), BindError> {
    match run(command, "cc") {
        Ok(()) => Ok(()),
        Err(BindError::ToolFailed { detail, .. }) => Err(BindError::AbiMismatch(detail)),
        Err(error) => Err(error),
    }
}

fn render_provenance(
    source_path: &Path,
    foreign_source: &str,
    generated_source: &str,
    copybook_path: &Path,
    copybook: &str,
    program: &str,
    layout: &RecordLayout,
    runtime_dir: &Path,
    archive: &Path,
) -> Result<String, BindError> {
    let descriptor = descriptor_stamp();
    let layout_facts = layout.fields.iter().map(|field| format!("{}:{}:{}:{:?}", field.name, field.offset, field.width, field.kind)).collect::<Vec<_>>().join(",");
    let mut identity = crate::ForeignBridge::IdentityBuilder::new("jet-cobol-bind-v1");
    identity.field("descriptor", descriptor.as_bytes());
    identity.field("source", foreign_source.as_bytes());
    identity.field("generated", generated_source.as_bytes());
    identity.field("copybook", copybook.as_bytes());
    identity.field("program", program.as_bytes());
    identity.field("layout", layout_facts.as_bytes());
    identity.field("runtime", runtime_dir.to_string_lossy().as_bytes());
    let archive_bytes = std::fs::read(archive).map_err(|error| BindError::IO(format!("could not read the COBOL archive for provenance: {error}")))?;
    Ok(format!(
        "schema=jet-cobol-bind-v1\nidentity={}\ndescriptor={}\nabi=C\nprogram={}\nsource_path={}\nsource_sha256={}\ngenerated_sha256={}\ncopybook_path={}\ncopybook_sha256={}\nrecord={}\nrecord_width={}\nfields={}\nruntime={}\narchive_sha256={}\n",
        identity.finish(),
        descriptor,
        program,
        source_path.display(),
        crate::SHA256::sha256_hex(foreign_source.as_bytes()),
        crate::SHA256::sha256_hex(generated_source.as_bytes()),
        copybook_path.display(),
        crate::SHA256::sha256_hex(copybook.as_bytes()),
        layout.name,
        layout.width,
        layout_facts,
        runtime_dir.display(),
        crate::SHA256::sha256_hex(&archive_bytes),
    ))
}

fn pic_digits(s: &str) -> Option<usize> {
    if s.is_empty() { return Some(0); }
    if s.bytes().all(|b| b == b'9') { return Some(s.len()); }
    s.strip_prefix("9(")?.strip_suffix(')')?.parse().ok()
}

fn parse_program_id(source: &str) -> Result<String, BindError> {
    source.lines().find_map(|l| { let u=l.trim().to_ascii_uppercase(); u.strip_prefix("PROGRAM-ID.").map(|v| v.trim().trim_end_matches('.').to_string()) })
        .filter(|v| !v.is_empty() && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .ok_or_else(|| BindError::Source("source has no valid PROGRAM-ID".into()))
}

fn render_jet(lib: &str, layout: &RecordLayout, packed: &FieldLayout, input: Option<&FieldLayout>) -> String {
    let abi = format!("jet_cobol_{lib}"); let descriptor = descriptor_stamp(); let mut out = format!("// jet-ffi-descriptor={descriptor}\n// cobol-layout {}: {} bytes\n", layout.name, layout.width);
    for f in &layout.fields { out.push_str(&format!("// cobol-field {}: offset={} width={} type={}{}\n", f.name, f.offset, f.width, f.kind.jet_type(), match f.kind { FieldKind::PackedDecimal { scale, .. } => format!(" scale={scale} encoding=COMP-3"), FieldKind::NativeInt { .. } => " encoding=COMP-5".into(), FieldKind::FixedText { .. } => " encoding=fixed-text".into() })); }
    out.push_str(&format!("#Codable\npub struct {} {{\n", upper_camel(&layout.name)));
    for f in &layout.fields { out.push_str(&format!("    {}: {}\n", f.name, f.kind.jet_type())); } out.push_str("}\n\n");
    out.push_str(&format!("#Extern module c.{abi} {{\n    fn apply_minor("));
    if input.is_some() { out.push_str("record_id: Int, "); } out.push_str(&format!("{}_minor: Int) Int = \"{abi}_apply_minor\"\n}}\nuse c.{abi} as abi\n\npub enum CobolError {{ Range ProgramFailed }}\n\n", packed.name));
    out.push_str("pub fn apply_minor("); if input.is_some() { out.push_str("record_id: Int, "); } out.push_str(&format!("{}_minor: Int) Int CobolError! -[FFI.Cobol]> {{\n", packed.name));
    if let Some(FieldKind::PackedDecimal { digits, signed, .. }) = Some(&packed.kind) { let max=10_i128.pow(*digits as u32)-1; let min=if *signed{-max}else{0}; out.push_str(&format!("    if {}_minor < {min} || {}_minor > {max} -> return Err(CobolError.Range)\n",packed.name,packed.name)); }
    if let Some(field)=input { if let FieldKind::NativeInt{digits,signed}=&field.kind {let max=10_i128.pow(*digits as u32)-1;let min=if *signed{-max}else{0};out.push_str(&format!("    if record_id < {min} || record_id > {max} -> return Err(CobolError.Range)\n"));}}
    out.push_str("    result :: abi.apply_minor("); if input.is_some() { out.push_str("record_id, "); } out.push_str(&format!("{}_minor)\n", packed.name));
    out.push_str("    if result == -9223372036854775808 -> return Err(CobolError.ProgramFailed)\n    return Ok(result)\n}\n"); out
}

fn render_bridge(program: &str, layout: &RecordLayout, packed: &FieldLayout, input: Option<&FieldLayout>, lib: &str) -> String {
    let args = if input.is_some() { "int64_t record_id, int64_t minor" } else { "int64_t minor" };
    let mut pre = String::new();
    if let Some(f) = input { let cty=match f.width {2=>"int16_t",4=>"int32_t",_=>"int64_t"}; pre=format!("  {cty} id = ({cty})record_id; memcpy(record + {}, &id, {});\n",f.offset,f.width); }
    format!(r#"#include <stdint.h>
#include <string.h>
#include <libcob.h>
#include <pthread.h>
extern int {program}(cob_u8_t *record);
static pthread_once_t jet_cobol_once = PTHREAD_ONCE_INIT;
static void jet_cobol_init(void) {{ cob_init(0, NULL); }}
static void pack(int64_t value, unsigned char *out, size_t n) {{
  uint64_t mag = value < 0 ? (uint64_t)(-(value + 1)) + 1 : (uint64_t)value;
  memset(out, 0, n); out[n-1] = (unsigned char)(value < 0 ? 0x0d : 0x0c);
  for (size_t pos = n * 2 - 1; pos > 0; --pos) {{ size_t nib = pos - 1; unsigned d=(unsigned)(mag%10); mag/=10; size_t b=nib/2; if(nib%2) out[b]|=(unsigned char)d; else out[b]|=(unsigned char)(d<<4); }}
}}
static int64_t unpack(const unsigned char *in, size_t n) {{
  int64_t v=0; for(size_t nib=0;nib<n*2-1;++nib){{ unsigned d=(nib%2)?(in[nib/2]&15):(in[nib/2]>>4); v=v*10+(int64_t)d; }} return ((in[n-1]&15)==0x0d)?-v:v;
}}
int64_t jet_cobol_{lib}_apply_minor({args}) {{
  cob_u8_t record[{record_width}]; memset(record, ' ', sizeof record);
{pre}  pack(minor, record + {packed_offset}, {packed_width});
  if(pthread_once(&jet_cobol_once, jet_cobol_init)!=0) return INT64_MIN;
  if({program}(record)!=0) return INT64_MIN;
  return unpack(record + {packed_offset}, {packed_width});
}}
"#, record_width=layout.width, packed_offset=packed.offset, packed_width=packed.width)
}

struct ToolOutput { status: std::process::ExitStatus, stdout: Vec<u8>, stderr: Vec<u8> }
fn run(command: &mut Command, tool: &'static str) -> Result<(), BindError> { let o=output(command,tool)?; if o.status.success(){Ok(())}else{Err(BindError::ToolFailed{tool,detail:launder(&o.stderr)})} }
fn checked_output(command: &mut Command, tool: &'static str) -> Result<ToolOutput, BindError> { let o=output(command,tool)?; if o.status.success(){Ok(o)}else{Err(BindError::ToolFailed{tool,detail:launder(&o.stderr)})} }
fn output(command: &mut Command, tool: &'static str) -> Result<ToolOutput, BindError> {
    const LIMIT:usize=64*1024; command.stdout(Stdio::piped()).stderr(Stdio::piped()); let mut child=command.spawn().map_err(|e|if e.kind()==std::io::ErrorKind::NotFound{BindError::ToolMissing(tool)}else{BindError::IO(format!("could not start `{tool}`: {e}"))})?;
    let stdout=child.stdout.take().ok_or_else(||BindError::IO(format!("could not supervise `{tool}` stdout")))?; let stderr=child.stderr.take().ok_or_else(||BindError::IO(format!("could not supervise `{tool}` stderr")))?;
    let out=std::thread::spawn(move||bounded(stdout,LIMIT)); let err=std::thread::spawn(move||bounded(stderr,LIMIT)); let deadline=Instant::now()+Duration::from_secs(60);
    let status=loop{match child.try_wait().map_err(|e|BindError::IO(format!("could not supervise `{tool}`: {e}")))?{Some(s)=>break s,None if Instant::now()>=deadline=>{let _=child.kill();let _=child.wait();let _=out.join();let _=err.join();return Err(BindError::ToolFailed{tool,detail:"the tool exceeded the 60 second limit".into()})},None=>std::thread::sleep(Duration::from_millis(10))}};
    let stdout=out.join().map_err(|_|BindError::IO(format!("`{tool}` stdout reader failed")))??; let stderr=err.join().map_err(|_|BindError::IO(format!("`{tool}` stderr reader failed")))??; Ok(ToolOutput{status,stdout,stderr})
}
fn bounded(mut input:impl Read,limit:usize)->Result<Vec<u8>,BindError>{let mut out=Vec::new();let mut buf=[0;8192];loop{let n=input.read(&mut buf).map_err(|e|BindError::IO(format!("could not read foreign tool output: {e}")))?;if n==0{break}let keep=limit.saturating_sub(out.len()).min(n);out.extend_from_slice(&buf[..keep]);}Ok(out)}
fn launder(_stderr:&[u8])->String{"the foreign tool returned a failure status".into()}
fn jet_name(s:&str)->String{s.trim_end_matches('.').to_ascii_lowercase().replace('-',"_")}
fn upper_camel(s:&str)->String{s.split('_').map(|p|{let mut c=p.chars();c.next().map(|h|h.to_ascii_uppercase().to_string()+c.as_str()).unwrap_or_default()}).collect()}
fn is_ident(s:&str)->bool{let mut c=s.chars();matches!(c.next(),Some(v)if v.is_ascii_alphabetic()||v=='_')&&c.all(|v|v.is_ascii_alphanumeric()||v=='_')}
fn descriptor_stamp()->String{crate::AST::binder_descriptor(crate::AST::ForeignLanguage::Cobol).expect("COBOL binder descriptor").stamp()}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn copybook_layout_keeps_packed_decimal_exact(){let l=parse_copybook("       01 PAYROLL-RECORD.\n          05 EMPLOYEE-ID PIC 9(6) COMP-5.\n          05 NAME PIC X(20).\n          05 GROSS-PAY PIC S9(7)V99 COMP-3.\n").unwrap();assert_eq!(l.width,29);assert_eq!(l.fields[2],FieldLayout{name:"gross_pay".into(),offset:24,width:5,kind:FieldKind::PackedDecimal{digits:9,scale:2,signed:true}});assert_eq!(l.fields[2].kind.jet_type(),"Decimal");}
    #[test] fn unsupported_layout_fails_instead_of_guessing(){assert!(parse_copybook("01 X.\n05 RATE PIC 9(4)V99 COMP-1.\n").is_err());}
    #[test] fn generated_surface_uses_current_arrows_and_typed_failure(){let l=parse_copybook("       01 X.\n          05 ID PIC 9(6) COMP-5.\n          05 AMOUNT PIC S9(7)V99 COMP-3.\n").unwrap();let source=render_jet("payroll",&l,&l.fields[1],Some(&l.fields[0]));assert!(source.contains("jet-ffi-descriptor="));assert!(source.contains("Int CobolError! -[FFI.Cobol]>"));assert!(source.contains("-> return Err(CobolError.Range)"));assert!(!source.contains("=>"));assert!(!source.contains(":[FFI.Cobol]"));}
    #[test] fn oversized_packed_decimal_fails_before_abi_codegen(){assert!(parse_copybook("01 X.\n05 AMOUNT PIC 9(19) COMP-3.\n").is_err());}
    #[test] fn free_format_and_closed_usage_parse_without_guessing(){let l=parse_copybook("01 record.\n  05 id PIC 9(4) COMP-5.\n  05 amount PIC S9(5)V99 COMP-3.\n").unwrap();assert_eq!(l.width,7);assert!(parse_copybook("01 record.\n05 amount PIC 9(4) BINARY.\n").is_err());assert!(parse_copybook("01 record.\n05 amount PIC 9(4) COMP-3 VALUE 1.\n").is_err());}
    #[test] fn reserved_copybook_names_fail_before_codegen(){assert!(parse_copybook("01 STRING.\n05 run PIC 9(4) COMP-5.\n").is_err());}
}
