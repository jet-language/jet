// Transport only: use the actual remaining runtime's attachment guard on
// real Rust-spawned threads. Identity/TCB assertions execute in Jet.
extern "C" {
    fn jet_rt_thread_fixture_parent() -> i64;
    fn jet_rt_thread_fixture_attached_child(parent: i64) -> i64;
    fn jet_rt_thread_fixture_attached() -> i64;
    fn jet_rt_thread_fixture_inherited_base(scratch: i64, expected: i64);
    fn jet_rt_thread_attach();
    fn jet_rt_thread_detach();
}

fn main() {
    let parent = unsafe { jet_rt_thread_fixture_parent() };
    let mut threads = Vec::with_capacity(64);
    for _ in 0..64 {
        threads.push(std::thread::spawn(move || {
            let _jet_thread = jet_runtime::JetNativeThreadAttachment::enter();
            unsafe { jet_rt_thread_fixture_attached_child(parent) };
        }));
    }
    for thread in threads {
        thread.join().expect("Rust thread transport failed");
    }
    // The child is created while the parent owns its TCB, but may not attach
    // until the parent's real guard has detached/freed it and the parent joins.
    // Exercise both fresh bootstrap and the explicit entryless-host API.
    for bootstrap in [true, false] {
        let (release, ready) = std::sync::mpsc::channel();
        let (publish, receive) = std::sync::mpsc::channel();
        let parent = std::thread::spawn(move || {
            let _jet_thread = jet_runtime::JetNativeThreadAttachment::enter();
            let old = unsafe { jet_rt_thread_fixture_parent() };
            let child = std::thread::spawn(move || {
                ready.recv().expect("parent release transport failed");
                let mut inherited = 0i64;
                unsafe {
                    jet_rt_thread_fixture_inherited_base(
                        (&mut inherited as *mut i64) as i64,
                        old,
                    );
                }
                if bootstrap {
                    let _jet_thread = jet_runtime::JetNativeThreadAttachment::enter();
                    unsafe { jet_rt_thread_fixture_attached() };
                } else {
                    unsafe {
                        jet_rt_thread_attach();
                        jet_rt_thread_fixture_attached();
                        jet_rt_thread_detach();
                    }
                }
            });
            publish.send(child).expect("child handle transport failed");
        });
        let child = receive.recv().expect("child handle transport failed");
        parent.join().expect("parent thread transport failed");
        release.send(()).expect("parent release transport failed");
        child.join().expect("delayed child transport failed");
    }
}
