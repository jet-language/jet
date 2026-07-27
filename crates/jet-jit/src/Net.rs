//! Minimal `core.net` hosts for watcher/port demos (#1219).
//! std::net only — same bind/local_addr surface as the CoreLib prelude.

use super::Concurrency;
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use std::cell::RefCell;
use std::net::{SocketAddr, TcpListener};

thread_local! {
    static LISTENERS: RefCell<Vec<Option<TcpListener>>> = const { RefCell::new(Vec::new()) };
    static ADDRS: RefCell<Vec<Option<SocketAddr>>> = const { RefCell::new(Vec::new()) };
}

fn result_ok_bits(bits: u64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.results.push(super::JitResultValue { ok: true, bits });
        rt.results.len() as i64
    })
}

fn result_err_msg(msg: &str) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let sid = rt.heap.alloc_string(msg.to_string());
        rt.results.push(super::JitResultValue {
            ok: false,
            bits: sid as u64,
        });
        rt.results.len() as i64
    })
}

fn clone_str(id: i64) -> String {
    Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(id).unwrap_or_default())
}

fn push_listener(listener: TcpListener) -> i64 {
    LISTENERS.with(|slot| {
        let mut v = slot.borrow_mut();
        v.push(Some(listener));
        v.len() as i64
    })
}

fn push_addr(addr: SocketAddr) -> i64 {
    ADDRS.with(|slot| {
        let mut v = slot.borrow_mut();
        v.push(Some(addr));
        v.len() as i64
    })
}

pub(crate) fn clear_net_state() {
    LISTENERS.with(|s| s.borrow_mut().clear());
    ADDRS.with(|s| s.borrow_mut().clear());
}

extern "C" fn jet_jit_net_tcp_listen(addr: i64) -> i64 {
    let addr = clone_str(addr);
    match TcpListener::bind(addr.as_str()) {
        Ok(listener) => {
            if let Err(e) = listener.set_nonblocking(true) {
                return result_err_msg(&e.to_string());
            }
            result_ok_bits(push_listener(listener) as u64)
        }
        Err(e) => result_err_msg(&e.to_string()),
    }
}

extern "C" fn jet_jit_net_listener_local_socket_addr(listener: i64) -> i64 {
    if listener <= 0 {
        return result_err_msg("invalid TcpListener");
    }
    let idx = (listener as usize).saturating_sub(1);
    let addr = LISTENERS.with(|slot| {
        slot.borrow()
            .get(idx)
            .and_then(|l| l.as_ref())
            .and_then(|l| l.local_addr().ok())
    });
    match addr {
        Some(addr) => result_ok_bits(push_addr(addr) as u64),
        None => result_err_msg("tcp listener local address failed"),
    }
}

extern "C" fn jet_jit_net_socket_port(addr: i64) -> i64 {
    if addr <= 0 {
        return 0;
    }
    let idx = (addr as usize).saturating_sub(1);
    ADDRS.with(|slot| {
        slot.borrow()
            .get(idx)
            .and_then(|a| a.as_ref())
            .map(|a| i64::from(a.port()))
            .unwrap_or(0)
    })
}

pub(crate) struct NetHostFns {
    pub tcp_listen: FuncId,
    pub listener_local_socket_addr: FuncId,
    pub socket_port: FuncId,
}

pub(crate) fn register_net_symbols(builder: &mut JITBuilder) {
    builder.symbol("jet_jit_net_tcp_listen", jet_jit_net_tcp_listen as *const u8);
    builder.symbol(
        "jet_jit_net_listener_local_socket_addr",
        jet_jit_net_listener_local_socket_addr as *const u8,
    );
    builder.symbol("jet_jit_net_socket_port", jet_jit_net_socket_port as *const u8);
}

pub(crate) fn declare_net_host_fns(module: &mut JITModule) -> Result<NetHostFns, String> {
    let cc = module.target_config().default_call_conv;
    let mut sig_unary = Signature::new(cc);
    sig_unary.params.push(AbiParam::new(types::I64));
    sig_unary.returns.push(AbiParam::new(types::I64));
    let mut import = |name: &str, sig: &Signature| -> Result<FuncId, String> {
        module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|e| e.to_string())
    };
    Ok(NetHostFns {
        tcp_listen: import("jet_jit_net_tcp_listen", &sig_unary)?,
        listener_local_socket_addr: import("jet_jit_net_listener_local_socket_addr", &sig_unary)?,
        socket_port: import("jet_jit_net_socket_port", &sig_unary)?,
    })
}
