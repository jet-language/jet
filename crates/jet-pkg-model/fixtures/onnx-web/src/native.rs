use jet_rt::model::provider::CancellationToken;
use std::future::Future;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    assert_eq!(args.len(), 3, "native REQUEST.bin LIBONNXRUNTIME RESULT.bin");
    let request = std::fs::read(&args[0]).expect("read request");
    let token = CancellationToken::new();
    if std::env::var_os("JET_ONNX_PROOF_CANCEL").is_some() { token.cancel(); }
    let mut future = std::pin::pin!(jet_onnx_web_proof::execute(request, Some(args[1].clone().into()), token));
    struct NativeWake;
    impl Wake for NativeWake { fn wake(self: Arc<Self>) {} }
    let waker = Waker::from(Arc::new(NativeWake));
    let Poll::Ready(result) = future.as_mut().poll(&mut Context::from_waker(&waker)) else {
        panic!("native provider unexpectedly suspended");
    };
    let result = result.unwrap_or_else(jet_onnx_web_proof::failure);
    std::fs::write(&args[2], result).expect("write result");
}
