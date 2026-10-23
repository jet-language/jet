// Core leaf identities come from the unified registry; provider ABIs are
// validated against the actual JetRuntime source, not inferred from names.
import { validateRuntimeRoute } from './runtime-route-abi.mjs';

const bindings = [
  ['core.sys.__env_value', 'jet_rt_logical_env_get_os', 'Bytes', 1, 3, 'i64'],
  ['core.sys.__env_set', 'jet_rt_logical_env_set', 'Set', 2, 2, 'i64'],
  ['core.sys.__env_remove', 'jet_rt_logical_env_unset', 'Remove', 1, 1, 'i64'],
  ['core.sys.__env_columns', 'jet_rt_logical_env_columns', 'Columns', 0, 1, null],
  ['core.sys.__startup_env_columns', 'jet_rt_startup_env_columns', 'Columns', 0, 1, null],
  ['core.sys.__env_key_compare', 'jet_rt_env_key_compare', 'Compare', 2, 2, 'i64'],
];

export function nativeEnvironmentRoutes(routes, exports, signatures) {
  const result = [];
  for (const [row, name, op, arity, physicalArity, ret] of bindings) {
    const symbols = [...routes].filter(([, route]) => route.rows.includes(row));
    if (symbols.length === 0) continue;
    if (symbols.length !== 1) throw new Error(`${row}: ambiguous native environment registry symbol`);
    if (!exports.has(name)) continue;
    validateRuntimeRoute({ symbol: row, name, params: Array.from({ length: physicalArity }, () => ({ ctys: ['i64'] })), result: { ret, outs: [] } }, signatures.get(name));
    result.push({ symbol: symbols[0][0], name, op, arity });
  }
  // These runtime consumers do not have a plain Core row.
  for (const [symbol, name, op, arity, ret] of [
    ['jet_std_env_init', 'jet_rt_logical_env_seed', 'Seed', 0, null],
    ['jet_rt_logical_env_get', 'jet_rt_logical_env_get', 'Text', 1, 'i64'],
  ]) {
    if (!exports.has(name)) continue;
    validateRuntimeRoute({ symbol, name, params: Array.from({ length: arity }, () => ({ ctys: ['i64'] })), result: { ret, outs: [] } }, signatures.get(name));
    result.push({ symbol, name, op, arity });
  }
  return result;
}

export function emitNativeEnvironmentRoutes(rows) {
  return [
    'enum IREnvironmentOp { Bytes Set Remove Columns Compare Seed Text }',
    'struct IREnvironmentRoute { symbol: String, export: String, op: IREnvironmentOp, arity: Int }',
    'IR_ENVIRONMENT_ROUTES :: prep { [IREnvironmentRoute]{',
    ...rows.map(row => `    IREnvironmentRoute{symbol: "${row.symbol}", export: "${row.name}", op: IREnvironmentOp.${row.op}, arity: ${row.arity}},`),
    '} }',
    'fn ir_environment_route(symbol: String) -> IREnvironmentRoute? {',
    '    loop route in IR_ENVIRONMENT_ROUTES { if route.symbol == symbol -> return Val(route.copy()) }',
    '    None',
    '}',
  ];
}
