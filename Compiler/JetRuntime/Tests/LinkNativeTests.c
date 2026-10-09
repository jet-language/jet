// The six generic Rust spellings are not physical exports. Exercise their
// already-existing, typed Lower-selected Jet providers without another shim.
#include <assert.h>
#include <stdint.h>
#include <stdio.h>

typedef int64_t W;
typedef struct { W len, cap, data; } Header;
typedef struct { W stride, clone, drop, show, equal, compare, kind, child0, child1; } Meta;
typedef struct { unsigned char *ptr; W len, cap; } Text;
extern W jet_rt_alloc(W, W);
extern void jet_rt_free(W, W, W);
extern void jet_rt_host_arguments(W, W, W);
extern void jet_rt_thread_attach(void);
extern W jet_rt_native_shared_new(W, W);
extern void jet_rt_native_shared_read(W, W, W, W);
extern void jet_rt_native_shared_edit(W, W, W, W);
extern void jet_rt_native_shared_drop(W);
extern void jet_rt_native_channel(W, W, W, W);
extern void jet_rt_native_sender_send(W, W, W);
extern W jet_rt_native_receiver_clone(W);
extern void jet_rt_native_sender_drop(W);
extern void jet_rt_native_receiver_drop(W);
extern void jet_rt_native_select_wait(W, W, W, W, W);
extern void jet_rt_native_atexit(W);
extern void jet_rt_task_exit_handlers(void);
extern W jet_rt_native_iter_from_list(W, W, W);
extern W jet_rt_native_iter_progress(W, W, W);
extern W jet_rt_native_iter_next(W);
extern void jet_rt_native_iter_drop(W);
static const Meta word_meta = {8,0,0,0,0,0,2,0,0};
static W addr(const void *p) { return (W)(uintptr_t)p; }
static W inspect(W env, W input, W output) {
    (void)env;
    *(W *)(uintptr_t)output = *(W *)(uintptr_t)input;
    return 0;
}
static W increment(W env, W input, W output) {
    (void)env;
    *(W *)(uintptr_t)output = ++*(W *)(uintptr_t)input;
    return 0;
}
static W exit_log, exit_drops;
static W exit_body(W closure) {
    exit_log = exit_log * 10 + ((W *)(uintptr_t)closure)[3];
    return 0;
}
static W exit_drop(W closure) {
    ++exit_drops;
    jet_rt_free(closure, 32, 8);
    return 0;
}
static W exit_closure(W digit) {
    W *closure = (W *)(uintptr_t)jet_rt_alloc(32, 8);
    closure[0] = 1;
    closure[1] = (W)(uintptr_t)exit_body;
    closure[2] = (W)(uintptr_t)exit_drop;
    closure[3] = digit;
    return addr(closure);
}
static Header receiver_list(W receiver) {
    W *data = (W *)(uintptr_t)jet_rt_alloc(8, 8);
    *data = jet_rt_native_receiver_clone(receiver);
    return (Header){1,1,addr(data)};
}
int main(int argc, char **argv, char **envp) {
    jet_rt_host_arguments(argc, addr(argv), addr(envp));
    jet_rt_thread_attach();
    W initial = 7, out = 0;
    W shared = jet_rt_native_shared_new(addr(&initial), addr(&word_meta));
    jet_rt_native_shared_read(shared, 0, (W)(uintptr_t)inspect, addr(&out));
    assert(out == 7);
    jet_rt_native_shared_edit(shared, 0, (W)(uintptr_t)increment, addr(&out));
    assert(out == 8);
    jet_rt_native_shared_read(shared, 0, (W)(uintptr_t)inspect, addr(&out));
    assert(out == 8 && initial == 7);
    jet_rt_native_shared_drop(shared);
    puts("shared read=7 edit=8 read=8");

    W tx, rx, payload = 42, arm = -99, selected = 0;
    jet_rt_native_channel(0, 0, addr(&tx), addr(&rx));
    jet_rt_native_sender_send(tx, addr(&payload), addr(&word_meta));
    Header recvs = receiver_list(rx), timers = {0};
    jet_rt_native_select_wait(addr(&recvs), addr(&timers), 0, addr(&arm), addr(&selected));
    assert(arm == 0 && selected && *(W *)(uintptr_t)selected == 42);
    assert(recvs.len == 0 && recvs.cap == 0 && recvs.data == 0);
    jet_rt_free(selected, 8, 8);
    recvs = receiver_list(rx);
    jet_rt_native_select_wait(addr(&recvs), addr(&timers), 1, addr(&arm), addr(&selected));
    assert(arm == -1 && selected == 0);
    jet_rt_native_sender_drop(tx);
    jet_rt_native_receiver_drop(rx);
    puts("select wait=0 value=42 probe=-1");

    jet_rt_native_atexit(exit_closure(1));
    jet_rt_native_atexit(exit_closure(2));
    jet_rt_task_exit_handlers();
    jet_rt_task_exit_handlers();
    assert(exit_log == 21 && exit_drops == 2);
    puts("atexit=21 drops=2");

    W *items = (W *)(uintptr_t)jet_rt_alloc(16, 8);
    items[0] = 4; items[1] = 5;
    Header source = {2,2,addr(items)};
    W iter = jet_rt_native_iter_from_list(addr(&source), addr(&word_meta), 1);
    Text description = {(unsigned char *)"work",4,0}, format = {(unsigned char *)"",0,0};
    W progress = jet_rt_native_iter_progress(iter, addr(&description), addr(&format));
    for (W expected = 4; expected <= 5; ++expected) {
        W slot = jet_rt_native_iter_next(progress);
        assert(slot && *(W *)(uintptr_t)slot == expected);
        jet_rt_free(slot, 8, 8);
    }
    assert(jet_rt_native_iter_next(progress) == 0);
    jet_rt_native_iter_drop(progress);
    assert(source.len == 0 && source.cap == 0 && source.data == 0);
    puts("progress=4,5 none=1");
    return 0;
}
