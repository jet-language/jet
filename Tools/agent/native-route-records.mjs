import { runtimeAbiAtom } from "./runtime-abi-symbols.mjs";
// Native record ABI metadata. Lower uses the declaration's real layout;
// these descriptors never define another header or duplicate the carrier codec.
const identifier = /^[A-Za-z_]\w*$/;
const carrier = /^(?:Text|Int|Flag|Float|Tag\.[A-Za-z_]\w*|Rec\.[A-Za-z_]\w*|Opt(?:Text|Int|Rec\.[A-Za-z_]\w*)|List\.(?:Byte|Rec\.[A-Za-z_]\w*)|Entries\.Rec\.[A-Za-z_]\w*)$/;

export function nativeRecordExports(types) {
  const exports = [];
  const seen = new Set();
  const add = (type, name, op, member = "") => {
    if (seen.has(name)) throw new Error(`native record export ${name}: duplicate ownership`);
    seen.add(name);
    exports.push({ name, runtime: type.runtime, jetName: type.jetName, op, member });
  };
  for (const type of types) {
    if (!identifier.test(type.runtime) || !type.jetName) throw new Error(`native record ${type.runtime}: missing declaration identity`);
    if (!type.source) continue;
    if (!type.source.path || !type.source.key) throw new Error(`native record ${type.runtime}: missing source declaration`);
    if (type.record) {
      if (!['struct', 'enum'].includes(type.record.kind)) throw new Error(`native record ${type.runtime}: unsupported declaration kind`);
      const members = type.record.members;
      if (!Array.isArray(members)) throw new Error(`native record ${type.runtime}: missing ordered members`);
      const names = new Set();
      for (const member of members) {
        if (!identifier.test(member.name) || names.has(member.name)) throw new Error(`native record ${type.runtime}: invalid or duplicate member ${member.name}`);
        names.add(member.name);
        if (member.kind !== null && !carrier.test(member.kind)) throw new Error(`native record ${type.runtime}.${member.name}: unsupported carrier ${member.kind}`);
        if (type.record.kind === 'struct' && member.kind === null) throw new Error(`native record ${type.runtime}.${member.name}: stored member has no carrier`);
      }
      if (type.record.kind === 'struct') {
        add(type, `jet_rt_record_new_${runtimeAbiAtom(type.runtime)}`, 'record.new');
        for (const member of members) add(type, `jet_rt_record_get_${runtimeAbiAtom(type.runtime)}_${runtimeAbiAtom(member.name)}`, 'record.get', member.name);
      } else {
        add(type, `jet_rt_record_tag_${runtimeAbiAtom(type.runtime)}`, 'record.tag');
        for (const member of members) {
          add(type, `jet_rt_record_new_${runtimeAbiAtom(type.runtime)}_${runtimeAbiAtom(member.name)}`, 'record.new', member.name);
          if (member.kind !== null) add(type, `jet_rt_record_get_${runtimeAbiAtom(type.runtime)}_${runtimeAbiAtom(member.name)}`, 'record.get', member.name);
        }
      }
    }
    for (const field of type.fields ?? []) {
      if (!identifier.test(field.name) || !/^(?:Text|Float|Flag|Handle\.[A-Za-z_]\w*)$/.test(field.kind)) throw new Error(`native field ${type.runtime}.${field.name}: unsupported address carrier ${field.kind}`);
      add(type, `jet_rt_handle_field_${runtimeAbiAtom(type.runtime)}_${runtimeAbiAtom(field.name)}`, 'record.field', field.name);
    }
  }
  // A collection holder has one owner regardless of how many records use it.
  const sequences = new Map();
  for (const type of types) {
    if (!type.source) continue;
    for (const member of type.record?.members ?? []) {
      if (!member.kind || !/^(?:List|Entries)\./.test(member.kind)) continue;
      const prior = sequences.get(member.kind);
      if (!prior) sequences.set(member.kind, type);
    }
  }
  for (const [kind, type] of [...sequences].sort(([a], [b]) => a.localeCompare(b))) {
    const prefix = `jet_rt_record_seq_${runtimeAbiAtom(kind.replaceAll('.', '_'))}`;
    for (const op of ['new', 'drop', 'len', 'push', 'get']) add(type, `${prefix}_${op}`, `record.seq.${op}`, kind);
    if (kind.startsWith('Entries.')) add(type, `${prefix}_key`, 'record.seq.key', kind);
  }
  return exports.sort((a, b) => a.name.localeCompare(b.name));
}

// callerMap contains only surviving raw-leaf/external consumers, never a
// historical lowering-use count. Empty consumer lists mean deletion.
export function partitionNativeRecordExports(types, callerMap) {
  const descriptors = nativeRecordExports(types);
  const synthesized = [], deleted = [];
  if (!(callerMap instanceof Map)) throw new Error("native record callers must be keyed by exact C symbol");
  for (const descriptor of descriptors) {
    const callers = callerMap.get(descriptor.name) ?? [];
    if (!Array.isArray(callers) || callers.some((caller) => typeof caller !== 'string' || caller.length === 0)) throw new Error(`native record export ${descriptor.name}: invalid caller witness`);
    if (callers.length > 0) synthesized.push({ ...descriptor, callers: [...new Set(callers)].sort() });
    else deleted.push(descriptor);
  }
  return { descriptors, synthesized, deleted };
}
