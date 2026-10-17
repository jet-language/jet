// Acceptance client of the actual Jet Shared/Condition/STM/Observe kernels.
// No protocol, scheduler, allocator or transaction implementation lives here.
#define _GNU_SOURCE
#include <assert.h>
#include <pthread.h>
#include <sched.h>
#include <stdatomic.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <time.h>
typedef uintptr_t W;
typedef struct { W stride, clone, drop, show, equal, compare, kind, child0, child1; } Meta;
extern void jet_rt_thread_attach(void), jet_rt_thread_detach(void);
extern W jet_rt_native_shared_new(W,const Meta*,W), jet_rt_native_shared_clone(W);
extern void jet_rt_native_shared_drop(W), jet_rt_native_shared_read(W,W,W,W);
extern W jet_rt_native_shared_guard_edit(W), jet_rt_native_shared_guard_read(W);
extern W jet_rt_native_shared_edit_guard_edit(W), jet_rt_native_shared_edit_guard_read(W);
extern W jet_rt_native_shared_read_guard_read(W), jet_rt_native_shared_read_guard_deref(W);
extern W jet_rt_native_shared_guard_revision(W);
extern void jet_rt_native_shared_edit_guard_drop(W), jet_rt_native_shared_read_guard_drop(W);
extern W jet_rt_native_condition_new(void), jet_rt_native_condition_clone(W);
extern void jet_rt_native_condition_drop(W), jet_rt_native_condition_notify_one(W), jet_rt_native_condition_notify_all(W);
extern W jet_rt_native_shared_guard_wait(W,W,W,W,W,W);
extern W jet_rt_shared_guard_acquire(W,W), jet_rt_shared_guard_wait_once(W,W);
extern bool jet_rt_shared_guard_held(W);
extern void jet_rt_shared_guard_drop(W);
extern W jet_rt_native_stm_begin(void);
extern void jet_rt_native_stm_commit(W), jet_rt_native_stm_rollback(W);
extern void jet_rt_native_shared_read_txn(W,W,W,W,W), jet_rt_native_shared_edit_txn(W,W,W,W,W);
extern W jet_rt_control_new(void), jet_rt_control_status(W);
extern void jet_rt_control_enter(W,W), jet_rt_control_leave(W), jet_rt_control_discard_stop(W);
extern void jet_rt_scheduler_control_cancel(W), jet_rt_scheduler_control_drop(W);
extern uint8_t jet_rt_control_publish_stop(W,W,W,W,W,W,W,W);
extern W jet_rt_sync_fixture_condition_count(W), jet_rt_sync_fixture_gate_depth(void);
extern void jet_rt_sync_fixture_condition_fifo(W), jet_rt_sync_fixture_set_revision(W,W);
extern W jet_rt_sync_fixture_cell_word(W,W);
extern W jet_rt_string_data(W), jet_rt_string_byte_len(W), jet_rt_string_from_static(W,W);
extern void jet_rt_string_drop(W);
extern W jet_rt_observe_parked_tasks_report(W,W), jet_rt_sync_fixture_observe_park(W,W,W,W);
extern void jet_rt_observe_task_finish(W), jet_rt_sync_fixture_observe_reset(void);
static _Atomic unsigned clones, drops, fail_clone;
static W clone_word(W input, W output) {
    unsigned ordinal = atomic_fetch_add(&clones,1) + 1;
    if (ordinal == atomic_load(&fail_clone)) {
        static const char message[] = "prepare clone rejected";
        assert(jet_rt_control_publish_stop(5,0,0,(W)-1,(W)-1,(W)message,sizeof(message)-1,0));
        return 5; // Deliberately does not initialize output.
    }
    *(W *)output = *(W *)input;
    return 0;
}
static W release_word(W input) { (void)input; atomic_fetch_add(&drops,1); return 0; }
static const Meta word_meta = {8,(W)clone_word,(W)release_word,0,0,0,2,0,0};
static W set_word(W env,W input,W output) { *(W *)input=env; *(W *)output=env; return 0; }
static W read_word(W env,W input,W output) { (void)env; *(W *)output=*(W *)input; return 0; }
static W cell_value(W cell) { W out=0; jet_rt_native_shared_read(cell,0,(W)read_word,(W)&out); return out; }
#if defined(JET_CONDITION_FIXTURE) || defined(JET_OBSERVE_FIXTURE)
static void text_is(W text,const char *expected) {
    assert(jet_rt_string_byte_len(text)==strlen(expected));
    assert(!memcmp((void *)jet_rt_string_data(text),expected,strlen(expected)));
}
#endif
static void clean_cell(W cell,W refs) {
    assert(jet_rt_sync_fixture_cell_word(cell,0)==refs);
    assert(jet_rt_sync_fixture_cell_word(cell,8)==0);
    assert(jet_rt_sync_fixture_cell_word(cell,24)==0);
    assert(jet_rt_sync_fixture_cell_word(cell,32)==0);
    assert(jet_rt_sync_fixture_cell_word(cell,40)==0);
    assert(jet_rt_sync_fixture_cell_word(cell,72)==0);
}
static void nested_stm(void) {
    W value=10,out=0,cell=jet_rt_native_shared_new((W)&value,&word_meta,(W)release_word);
    unsigned before=atomic_load(&clones), before_drop=atomic_load(&drops);
    W parent=jet_rt_native_stm_begin();
    jet_rt_native_shared_edit_txn(cell,parent,20,(W)set_word,(W)&out);
    W child=jet_rt_native_stm_begin();
    jet_rt_native_shared_read_txn(cell,child,0,(W)read_word,(W)&out); assert(out==20);
    jet_rt_native_shared_edit_txn(cell,child,30,(W)set_word,(W)&out);
    jet_rt_native_stm_commit(child);
    // Only first-touch parent and child clones; nested merge transfers, not clones.
    assert(atomic_load(&clones)==before+2);
    jet_rt_native_shared_read_txn(cell,parent,0,(W)read_word,(W)&out); assert(out==30);
    assert(cell_value(cell)==10);
    W rolled=jet_rt_native_stm_begin();
    jet_rt_native_shared_edit_txn(cell,rolled,99,(W)set_word,(W)&out);
    jet_rt_native_stm_rollback(rolled);
    jet_rt_native_shared_read_txn(cell,parent,0,(W)read_word,(W)&out); assert(out==30);
    jet_rt_native_stm_commit(parent);
    assert(atomic_load(&clones)==before+4); // includes observable commit clone.
    assert(cell_value(cell)==30);
    assert(jet_rt_sync_fixture_cell_word(cell,48)==1);
    clean_cell(cell,1); assert(jet_rt_sync_fixture_gate_depth()==0);
    jet_rt_native_shared_drop(cell);
    assert(atomic_load(&drops)==before_drop+5); // parent, rollback, stage, retired, final.
    puts("STM nested commit transfer/rollback/clone parity ok");
}
typedef struct { W cell,expected; _Atomic unsigned attempted,entered,release; } Gate;
static void *gate_worker(void *argument) {
    jet_rt_thread_attach(); Gate *g=argument;
    atomic_store(&g->attempted,1);
    W state=jet_rt_native_stm_begin(); atomic_store(&g->entered,1);
    W out=0; jet_rt_native_shared_read_txn(g->cell,state,0,(W)read_word,(W)&out); assert(out==g->expected);
    jet_rt_native_shared_edit_txn(g->cell,state,g->expected+1,(W)set_word,(W)&out);
    jet_rt_native_stm_commit(state); jet_rt_thread_detach(); return NULL;
}
static void gate_stm(void) {
    W value=40,out=0,cell=jet_rt_native_shared_new((W)&value,&word_meta,(W)release_word);
    W state=jet_rt_native_stm_begin();
    jet_rt_native_shared_edit_txn(cell,state,41,(W)set_word,(W)&out);
    Gate g={.cell=cell,.expected=41}; pthread_t worker; assert(!pthread_create(&worker,NULL,gate_worker,&g));
    while (!atomic_load(&g.attempted)) sched_yield();
    struct timespec delay={0,20000000}; nanosleep(&delay,NULL);
    assert(!atomic_load(&g.entered)); assert(jet_rt_sync_fixture_gate_depth()==1);
    jet_rt_native_stm_commit(state); assert(!pthread_join(worker,NULL));
    assert(atomic_load(&g.entered)); assert(cell_value(cell)==42);
    assert(jet_rt_sync_fixture_cell_word(cell,48)==2);
    clean_cell(cell,1); assert(jet_rt_sync_fixture_gate_depth()==0);
    jet_rt_native_shared_drop(cell); puts("STM two-thread body gate serialization ok");
}
// The compiler's uncommitted Stm drop calls rollback, not commit. A second
// attached thread must acquire the root gate and see no abandoned publication.
static void dropped_root_stm(void) {
    W value=40,out=0,cell=jet_rt_native_shared_new((W)&value,&word_meta);
    W state=jet_rt_native_stm_begin();
    jet_rt_native_shared_edit_txn(cell,state,999,(W)set_word,(W)&out);
    Gate g={.cell=cell,.expected=40}; pthread_t worker; assert(!pthread_create(&worker,NULL,gate_worker,&g));
    while (!atomic_load(&g.attempted)) sched_yield();
    struct timespec delay={0,20000000}; nanosleep(&delay,NULL);
    assert(!atomic_load(&g.entered)); assert(jet_rt_sync_fixture_gate_depth()==1);
    jet_rt_native_stm_rollback(state); assert(!pthread_join(worker,NULL));
    assert(atomic_load(&g.entered)); assert(cell_value(cell)==41);
    assert(jet_rt_sync_fixture_cell_word(cell,48)==1);
    clean_cell(cell,1); assert(jet_rt_sync_fixture_gate_depth()==0);
    jet_rt_native_shared_drop(cell); puts("STM dropped uncommitted root releases second-thread gate ok");
}
static void rejected_stm(void) {
    W save[4],control=jet_rt_control_new(); jet_rt_control_enter(control,(W)save);
    W value=1,out=0,a=jet_rt_native_shared_new((W)&value,&word_meta,(W)release_word),b=jet_rt_native_shared_new((W)&value,&word_meta,(W)release_word);
    unsigned before_drop=atomic_load(&drops);
    W state=jet_rt_native_stm_begin();
    jet_rt_native_shared_edit_txn(a,state,2,(W)set_word,(W)&out);
    jet_rt_native_shared_edit_txn(b,state,3,(W)set_word,(W)&out);
    // Let one prepare succeed and reject the next: neither may publish.
    atomic_store(&fail_clone,atomic_load(&clones)+2);
    jet_rt_native_stm_commit(state); assert(jet_rt_control_status(control)==5);
    atomic_store(&fail_clone,0); jet_rt_control_discard_stop(control);
    assert(cell_value(a)==1 && cell_value(b)==1);
    assert(jet_rt_sync_fixture_cell_word(a,48)==0 && jet_rt_sync_fixture_cell_word(b,48)==0);
    clean_cell(a,1); clean_cell(b,1); assert(jet_rt_sync_fixture_gate_depth()==0);
    assert(atomic_load(&drops)==before_drop+3); // two stages + initialized prepare.
    state=jet_rt_native_stm_begin();
    jet_rt_native_shared_edit_txn(a,state,7,(W)set_word,(W)&out);
    jet_rt_sync_fixture_set_revision(a,(W)-1);
    jet_rt_native_stm_commit(state); assert(jet_rt_control_status(control)==5);
    jet_rt_control_discard_stop(control);
    assert(cell_value(a)==1 && jet_rt_sync_fixture_cell_word(a,48)==(W)-1);
    clean_cell(a,1); assert(jet_rt_sync_fixture_gate_depth()==0);
    W guard=jet_rt_native_shared_guard_read(a); jet_rt_native_shared_read_guard_drop(guard);
    jet_rt_native_shared_drop(a); jet_rt_native_shared_drop(b);
    jet_rt_control_leave((W)save); jet_rt_scheduler_control_drop(control);
    puts("STM prepare rejection/no publication/drop/revision stop ok");
}
#ifdef JET_CONDITION_FIXTURE
typedef struct { W cell,condition,control; _Atomic unsigned done; unsigned cancel; } Wait;
static W ready_word(W env,W input,W output) { (void)env; *(uint8_t *)output=*(W *)input!=0; return 0; }
static W stale_ready(W env,W input,W output) { (void)input; unsigned *calls=(unsigned *)env; *(uint8_t *)output=++*calls==2; return 0; }
static void *condition_waiter(void *argument) {
    jet_rt_thread_attach(); Wait *w=argument; W save[4]; jet_rt_control_enter(w->control,(W)save);
    W token=jet_rt_native_shared_guard_edit(w->cell),failure=0;
    W result=jet_rt_native_shared_guard_wait((W)&token,1,w->condition,0,(W)ready_word,(W)&failure);
    if (w->cancel) {
        assert(result==1 && jet_rt_control_status(w->control)==1 && token==(w->cell|1));
        jet_rt_control_discard_stop(w->control);
        // Each accessor refuses an unheld token before any payload access.
        W (*accessors[])(W)={jet_rt_native_shared_edit_guard_read,jet_rt_native_shared_edit_guard_edit,jet_rt_native_shared_read_guard_read,jet_rt_native_shared_read_guard_deref,jet_rt_native_shared_guard_revision};
        for (unsigned i=0;i<5;i++) { assert(accessors[i](token)==0); assert(jet_rt_control_status(w->control)==5); jet_rt_control_discard_stop(w->control); }
        jet_rt_native_shared_edit_guard_drop(token);
        // Another writer is still held by the main thread. Unheld drop MUST
        // not clear its permit or commit its revision. Ordinary guards own a
        // logical alias, not a lifetime physical borrow.
        assert(jet_rt_sync_fixture_cell_word(w->cell,32)==1);
        assert(jet_rt_sync_fixture_cell_word(w->cell,72)==0);
        assert(jet_rt_sync_fixture_cell_word(w->cell,48)==0);
    } else {
        assert(result==0 && jet_rt_control_status(w->control)==0 && token==w->cell);
        jet_rt_native_shared_edit_guard_drop(token);
    }
    atomic_store(&w->done,1); jet_rt_control_leave((W)save); jet_rt_thread_detach(); return NULL;
}
static void *physical_condition_waiter(void *argument) {
    jet_rt_thread_attach();
    Wait *w=argument;
    W save[4];
    jet_rt_control_enter(w->control,(W)save);
    W guard=jet_rt_shared_guard_acquire(w->cell,1);
    assert(guard && jet_rt_shared_guard_held(guard));
    W result=jet_rt_shared_guard_wait_once(guard,w->condition);
    assert(result==1 && jet_rt_control_status(w->control)==1);
    assert(!jet_rt_shared_guard_held(guard));
    assert(jet_rt_sync_fixture_condition_count(w->condition)==0);
    jet_rt_control_discard_stop(w->control);
    jet_rt_shared_guard_drop(guard);
    assert(jet_rt_sync_fixture_condition_count(w->condition)==0);
    atomic_store(&w->done,1);
    jet_rt_control_leave((W)save);
    jet_rt_thread_detach();
    return NULL;
}
static void condition_tests(void) {
    W condition=jet_rt_native_condition_new(); W alias=jet_rt_native_condition_clone(condition);
    jet_rt_native_condition_drop(alias); jet_rt_sync_fixture_condition_fifo(condition);
    W value=0,cell=jet_rt_native_shared_new((W)&value,&word_meta,(W)release_word),failure=0;
    W read=jet_rt_native_shared_guard_read(cell);
    assert(jet_rt_native_shared_guard_wait((W)&read,0,condition,0,(W)ready_word,(W)&failure)==1);
    text_is(failure,"a condition wait needs an edit guard"); jet_rt_string_drop(failure);
    jet_rt_native_shared_read_guard_drop(read);
    W guard=jet_rt_native_shared_guard_edit(cell); unsigned calls=0;
    jet_rt_native_condition_notify_one(condition);
    assert(jet_rt_native_shared_guard_wait((W)&guard,1,condition,(W)&calls,(W)stale_ready,(W)&failure)==0);
    assert(calls==2 && guard==cell); jet_rt_native_shared_edit_guard_drop(guard);
    // Reset revision for the separate cancellation-cleanup acceptance case.
    jet_rt_sync_fixture_set_revision(cell,0);
    Wait w={.cell=cell,.condition=condition,.control=jet_rt_control_new(),.cancel=1};
    pthread_t worker; assert(!pthread_create(&worker,NULL,condition_waiter,&w));
    while (jet_rt_sync_fixture_condition_count(condition)!=1) sched_yield();
    guard=jet_rt_native_shared_guard_edit(cell);
    jet_rt_scheduler_control_cancel(w.control);
    while (!atomic_load(&w.done)) sched_yield();
    assert(!pthread_join(worker,NULL));
    assert(jet_rt_sync_fixture_condition_count(condition)==0);
    assert(jet_rt_sync_fixture_cell_word(cell,0)==2);
    jet_rt_native_shared_edit_guard_drop(guard); clean_cell(cell,1);
    jet_rt_scheduler_control_drop(w.control);
    // The physical Guard40 wait uses the SAME condition queue. Cancellation
    // removes its sole registration before leaving its lease unheld.
    jet_rt_sync_fixture_set_revision(cell,0);
    w=(Wait){.cell=cell,.condition=condition,.control=jet_rt_control_new(),.cancel=1};
    assert(!pthread_create(&worker,NULL,physical_condition_waiter,&w));
    while (jet_rt_sync_fixture_condition_count(condition)!=1) sched_yield();
    jet_rt_scheduler_control_cancel(w.control);
    assert(!pthread_join(worker,NULL));
    assert(atomic_load(&w.done));
    assert(jet_rt_sync_fixture_condition_count(condition)==0);
    assert(jet_rt_sync_fixture_cell_word(cell,48)==0);
    clean_cell(cell,1);
    jet_rt_scheduler_control_drop(w.control);
    w=(Wait){.cell=cell,.condition=condition,.control=jet_rt_control_new()};
    assert(!pthread_create(&worker,NULL,condition_waiter,&w));
    while (jet_rt_sync_fixture_condition_count(condition)!=1) sched_yield();
    guard=jet_rt_native_shared_guard_edit(cell); *(W *)jet_rt_native_shared_edit_guard_edit(guard)=1;
    jet_rt_native_condition_notify_all(condition); jet_rt_native_shared_edit_guard_drop(guard);
    assert(!pthread_join(worker,NULL)); assert(atomic_load(&w.done)); clean_cell(cell,1);
    jet_rt_scheduler_control_drop(w.control); jet_rt_native_shared_drop(cell); jet_rt_native_condition_drop(condition);
    puts("Condition FIFO/drain/pending/wait/cancel/unheld access+drop ok");
}
#endif
#ifdef JET_OBSERVE_FIXTURE
static W text(const char *s) { return jet_rt_string_from_static((W)s,strlen(s)); }
static void observe_report(void) {
    W rendered=0x1234,status=0x5678; jet_rt_sync_fixture_observe_reset();
    assert(jet_rt_observe_parked_tasks_report((W)&rendered,(W)&status)==0);
    assert(rendered==0x1234 && status==0x5678);
    W control=jet_rt_control_new(),label=text("kid\"\\\n"),wait=text("channel recv");
    W id=jet_rt_sync_fixture_observe_park(control,7,label,wait); jet_rt_sync_fixture_observe_reset();
    assert(jet_rt_observe_parked_tasks_report((W)&rendered,(W)&status)==1 && status==70);
    text_is(rendered,"Stop [E3013]: Parked tasks remain at process exit: \n  kid\\\"\\\\  (spawn site 7)\n    state: blocked\n    wait target: channel recv\n Why: The scheduler reached process exit while one or more tasks were blocked on a wait target.\n Fix: Join each task or make its wait reachable before process exit.\nMore: jet-lang.dev/e/E3013\n");
    jet_rt_string_drop(rendered); rendered=0x1234; status=0x5678;
    assert(jet_rt_observe_parked_tasks_report((W)&rendered,(W)&status)==0);
    assert(rendered==0x1234 && status==0x5678);
    jet_rt_observe_task_finish(id); jet_rt_string_drop(label); jet_rt_string_drop(wait); jet_rt_scheduler_control_drop(control);
    puts("Observe empty/parked exact report/owned once ok");
}
#endif
int main(void) {
    jet_rt_thread_attach(); nested_stm(); gate_stm(); rejected_stm();
    dropped_root_stm();
#ifdef JET_CONDITION_FIXTURE
    condition_tests();
#endif
#ifdef JET_OBSERVE_FIXTURE
    observe_report();
#endif
    jet_rt_thread_detach(); return 0;
}
