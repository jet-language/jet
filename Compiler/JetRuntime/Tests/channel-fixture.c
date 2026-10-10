// Raw-slot acceptance and paired performance client. Both executables call
// the actual runtime exports; no scheduler/channel/Shared implementation lives
// here. Native callbacks operate only on their borrowed descriptor slots.
#define _GNU_SOURCE
#include <assert.h>
#include <inttypes.h>
#include <pthread.h>
#include <stdatomic.h>
#include <sched.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

typedef uintptr_t W;
typedef struct { W len, cap, data; } Header;
typedef struct { W stride, clone, drop, show, equal, compare, kind, child0, child1; } Meta;
extern W jet_rt_alloc(W,W);
extern void jet_rt_free(W,W,W);
extern void jet_rt_thread_attach(void);
extern void jet_rt_thread_detach(void);
#ifdef JET_SEED
// The Rust seed uses its real FS TLS, not the Jet native GS implementation.
#define jet_rt_thread_attach() ((void)0)
#define jet_rt_thread_detach() ((void)0)
#endif
extern void jet_rt_native_channel(uint8_t,int64_t,W*,W*);
extern void jet_rt_native_sender_send(W,W,const Meta*);
extern void jet_rt_native_sender_close(W);
extern W jet_rt_native_sender_clone(W);
extern void jet_rt_native_sender_drop(W);
extern void jet_rt_native_receiver_close(W);
extern uint8_t jet_rt_native_receiver_is_ready(W);
extern W jet_rt_native_receiver_try_receive(W,const Meta*);
extern int64_t jet_rt_native_receiver_receive(W,W);
extern W jet_rt_native_receiver_clone(W);
extern void jet_rt_native_receiver_drop(W);
extern W jet_rt_native_receiver_iter(W,uint8_t,const Meta*);
extern void jet_rt_native_iter_drop(W);
extern void jet_rt_native_select_wait(Header*,Header*,uint8_t,int64_t*,W*);
#ifdef JET_SEED
// Seed-only client of the Rust provider: this ABI dies at native cutover.
extern W jet_rt_native_shared_new(W,const Meta*);
#else
extern W jet_rt_native_shared_new(W,const Meta*,W);
#endif
extern W jet_rt_native_shared_clone(W);
extern void jet_rt_native_shared_drop(W);
extern void jet_rt_native_shared_read(W,W,W,W);
extern void jet_rt_native_shared_edit(W,W,W,W);
extern W jet_rt_handle_clone_closed(W);
extern void jet_rt_handle_drop_closed(W);
extern uint8_t jet_rt_handle_eq_closed(W,W);
extern W jet_rt_handle_show_closed(W);
extern W jet_rt_string_data(W);
extern W jet_rt_string_byte_len(W);
extern void jet_rt_string_drop(W);
#ifdef JET_CLOSED_CROSS
extern W jet_fixture_rust_closed_new(void);
extern void jet_fixture_rust_closed_drop(W);
extern W jet_fixture_rust_closed_show(W);
static void closed_cross(void) {
    W from_rust = jet_fixture_rust_closed_new();
    assert(from_rust == 1);
    W shown = jet_rt_handle_show_closed(from_rust);
    assert(jet_rt_string_byte_len(shown) == 6);
    assert(!memcmp((void *)jet_rt_string_data(shown), "Closed", 6));
    jet_rt_string_drop(shown);
    jet_rt_handle_drop_closed(from_rust);
    W from_jet = jet_rt_handle_clone_closed(1);
    shown = jet_fixture_rust_closed_show(from_jet);
    assert(jet_rt_string_byte_len(shown) == 6);
    assert(!memcmp((void *)jet_rt_string_data(shown), "Closed", 6));
    jet_rt_string_drop(shown);
    jet_fixture_rust_closed_drop(from_jet);
    puts("Closed Rust/Jet ownership cross ok");
}
#endif
#ifdef JET_CHANNEL_INTERNAL
extern void jet_rt_channel_fixture_reservations(const Meta *);
extern W jet_rt_channel_fixture_stop(W,W,W);
extern void jet_rt_channel_fixture_stops(const Meta *,W,const Meta *,W);
extern W jet_rt_channel_fixture_writers_waiting(W);
extern W jet_rt_channel_fixture_select_wake_handoff(const Meta *);
extern void jet_rt_channel_fixture_shared_guard_stops(const Meta *);
extern W jet_rt_native_shared_guard_read(W);
extern W jet_rt_native_shared_guard_edit(W);
extern W jet_rt_native_shared_read_guard_read(W);
extern W jet_rt_native_shared_read_guard_deref(W);
extern W jet_rt_native_shared_edit_guard_read(W);
extern W jet_rt_native_shared_edit_guard_edit(W);
extern W jet_rt_native_shared_guard_revision(W);
extern void jet_rt_native_shared_read_guard_drop(W);
extern void jet_rt_native_shared_edit_guard_drop(W);
#endif
#if defined(JET_SEED) || defined(JET_CLOSED_CROSS)
extern uint8_t jet_fixture_rust_select_wake_handoff(void);
#endif

static const Meta word_meta = {8,0,0,0,0,0,2,0,0};
static W fixture_shared_new(W slot, const Meta *meta, W drop_code) {
#ifdef JET_SEED
    (void)drop_code;
    return jet_rt_native_shared_new(slot, meta);
#else
    return jet_rt_native_shared_new(slot, meta, drop_code);
#endif
}
static atomic_uint drops;
static void payload_drop(W p) { (void)p; atomic_fetch_add_explicit(&drops,1,memory_order_relaxed); }
static const Meta owned_meta = {8,0,(W)payload_drop,0,0,0,2,0,0};
static uint64_t inspect(W,W,W);
static uint64_t now_ns(void) { struct timespec t; assert(!clock_gettime(CLOCK_MONOTONIC,&t)); return (uint64_t)t.tv_sec*1000000000u+(uint64_t)t.tv_nsec; }
static W receive(W rx) { W out=0; assert(jet_rt_native_receiver_receive(rx,(W)&out)==1); return out; }
static Header words(const W *p,size_t n) { Header h={n,n,0}; if(n){h.data=jet_rt_alloc(n*8,8);memcpy((void*)h.data,p,n*8);}return h; }
static void closed(W rx) { W out=0; assert(jet_rt_native_receiver_receive(rx,(W)&out)==0); assert(out==1); assert(jet_rt_handle_clone_closed(out)==1); assert(jet_rt_handle_eq_closed(out,1)); W text=jet_rt_handle_show_closed(out); assert(jet_rt_string_byte_len(text)==6); assert(!memcmp((void*)jet_rt_string_data(text),"Closed",6));jet_rt_string_drop(text);jet_rt_handle_drop_closed(out); }

static void queue_tests(void) {
#ifdef JET_CLOSED_CROSS
    closed_cross();
#endif
#ifdef JET_CHANNEL_INTERNAL
    jet_rt_channel_fixture_reservations(&word_meta);
    assert(jet_rt_channel_fixture_select_wake_handoff(&word_meta) == 1);
    puts("select wake handoff parity ok");
    unsigned cancelled_drops = atomic_load(&drops);
    jet_rt_channel_fixture_stops(&word_meta, (W)jet_rt_channel_fixture_stop,
                                &owned_meta, (W)inspect);
    assert(atomic_load(&drops) == cancelled_drops + 1);
    puts("reservations/stopped output/permit cleanup ok");
#endif
#if defined(JET_SEED) || defined(JET_CLOSED_CROSS)
    assert(jet_fixture_rust_select_wake_handoff() == 1);
    puts("select wake handoff parity ok");
#endif
    W tx,rx;jet_rt_native_channel(1,2,&tx,&rx);
    W alias=jet_rt_native_sender_clone(tx),rxa=jet_rt_native_receiver_clone(rx);
    for(W i=0;i<10000;i+=2){W a=i,b=i+1;jet_rt_native_sender_send(tx,(W)&a,&word_meta);jet_rt_native_sender_send(alias,(W)&b,&word_meta);assert(jet_rt_native_receiver_is_ready(rx));assert(receive(rxa)==i);W option=jet_rt_native_receiver_try_receive(rx,&word_meta);assert(option&&*(W*)option==i+1);jet_rt_free(option,8,8);assert(!jet_rt_native_receiver_try_receive(rx,&word_meta));}
    jet_rt_native_sender_drop(tx);assert(!jet_rt_native_receiver_is_ready(rx));
    W value=7;jet_rt_native_sender_send(alias,(W)&value,&word_meta);jet_rt_native_sender_drop(alias);assert(receive(rx)==7);closed(rx);assert(jet_rt_native_receiver_is_ready(rx));jet_rt_native_receiver_drop(rxa);jet_rt_native_receiver_drop(rx);
    jet_rt_native_channel(0,0,&tx,&rx);
    unsigned before=atomic_load(&drops);for(W i=0;i<11;i++)jet_rt_native_sender_send(tx,(W)&i,&owned_meta);
    jet_rt_native_receiver_close(rx);W rejected=99;jet_rt_native_sender_send(tx,(W)&rejected,&owned_meta);assert(atomic_load(&drops)==before+1);jet_rt_native_sender_drop(tx);jet_rt_native_receiver_drop(rx);assert(atomic_load(&drops)==before+12);
    jet_rt_native_channel(0,0,&tx,&rx);
    W iter=jet_rt_native_receiver_iter(rx,0,&word_meta);jet_rt_native_iter_drop(iter);jet_rt_native_sender_close(tx);closed(rx);jet_rt_native_sender_drop(tx);jet_rt_native_receiver_drop(rx);
    puts("queue/close/owners ok");
}

static void select_tests(void) {
    W tx[2],rx[2];for(size_t i=0;i<2;i++)jet_rt_native_channel(0,0,&tx[i],&rx[i]);
    W a=10,b=20;jet_rt_native_sender_send(tx[0],(W)&a,&word_meta);jet_rt_native_sender_send(tx[1],(W)&b,&word_meta);
    W handles[]={jet_rt_native_receiver_clone(rx[0]),jet_rt_native_receiver_clone(rx[1])};W zero[]={0};Header rs=words(handles,2),ts=words(zero,1);int64_t arm=-99;W out=0;
    jet_rt_native_select_wait(&rs,&ts,0,&arm,&out);assert(!rs.len&&!rs.cap&&!rs.data&&!ts.len&&!ts.cap&&!ts.data);assert(arm==0&&out&&*(W*)out==10);jet_rt_free(out,8,8);
    handles[0]=jet_rt_native_receiver_clone(rx[0]);handles[1]=jet_rt_native_receiver_clone(rx[1]);rs=words(handles,2);ts=words(zero,1);jet_rt_native_select_wait(&rs,&ts,1,&arm,&out);assert(arm==1&&out&&*(W*)out==20);jet_rt_free(out,8,8);
    handles[0]=jet_rt_native_receiver_clone(rx[0]);rs=words(handles,1);ts=words(zero,1);jet_rt_native_select_wait(&rs,&ts,0,&arm,&out);assert(arm==1&&!out);
    handles[0]=jet_rt_native_receiver_clone(rx[0]);rs=words(handles,1);ts=words(NULL,0);jet_rt_native_select_wait(&rs,&ts,1,&arm,&out);assert(arm==-1&&!out);
    W timers[]={5000000,0};rs=words(NULL,0);ts=words(timers,2);jet_rt_native_select_wait(&rs,&ts,0,&arm,&out);assert(arm==1&&!out);
    W timer[]={2000000};rs=words(NULL,0);ts=words(timer,1);uint64_t start=now_ns();jet_rt_native_select_wait(&rs,&ts,0,&arm,&out);assert(arm==0&&!out);assert(now_ns()-start>=1000000);
    for(size_t i=0;i<2;i++){jet_rt_native_sender_drop(tx[i]);jet_rt_native_receiver_drop(rx[i]);}
    puts("select source-order/probe/timers ok");
}

typedef struct { W tx,rx,rounds; } Ping;
static void *producer(void *p) {jet_rt_thread_attach();Ping *s=p;for(W i=1;i<=s->rounds;i++)jet_rt_native_sender_send(s->tx,(W)&i,&word_meta);jet_rt_native_sender_drop(s->tx);jet_rt_thread_detach();return NULL;}
static void ping(W rounds) { W tx,rx;jet_rt_native_channel(1,1,&tx,&rx);Ping s={tx,rx,rounds};pthread_t worker;uint64_t start=now_ns();assert(!pthread_create(&worker,NULL,producer,&s));uint64_t sum=0;for(W i=1;i<=rounds;i++){W value=receive(rx);assert(value==i);sum+=value;}closed(rx);assert(!pthread_join(worker,NULL));jet_rt_native_receiver_drop(rx);printf("ping rounds=%"PRIuPTR" checksum=%"PRIu64" ns=%"PRIu64"\n",rounds,sum,now_ns()-start); }

static uint64_t increment(W env,W input,W output){(void)env;*(W*)input+=1;*(W*)output=*(W*)input;return 0;}
static uint64_t inspect(W env,W input,W output){(void)env;*(W*)output=*(W*)input;return 0;}
typedef struct { W shared,rounds; } Edits;
static void *editor(void *p){jet_rt_thread_attach();Edits *e=p;W out;for(W i=0;i<e->rounds;i++)jet_rt_native_shared_edit(e->shared,0,(W)increment,(W)&out);jet_rt_native_shared_drop(e->shared);jet_rt_thread_detach();return NULL;}
static void shared_tests(void) {
    W value = 0, shared = fixture_shared_new((W)&value, &word_meta, 0);
    pthread_t threads[4];
    Edits args[4];
    for (size_t i = 0; i < 4; i++) {
        args[i] = (Edits){jet_rt_native_shared_clone(shared), 25000};
        assert(!pthread_create(&threads[i], NULL, editor, &args[i]));
    }
    for (size_t i = 0; i < 4; i++) assert(!pthread_join(threads[i], NULL));
    W out = 0;
    jet_rt_native_shared_read(shared, 0, (W)inspect, (W)&out);
    assert(out == 100000);
    jet_rt_native_shared_drop(shared);
    unsigned before = atomic_load(&drops);
    shared = fixture_shared_new((W)&value, &owned_meta, (W)payload_drop);
    W alias = jet_rt_native_shared_clone(shared);
    jet_rt_native_shared_drop(shared);
    assert(atomic_load(&drops) == before);
    jet_rt_native_shared_drop(alias);
    assert(atomic_load(&drops) == before + 1);
#ifndef JET_SEED
    // No descriptor-drop fallback: null glue is a plain-free contract.
    shared = fixture_shared_new((W)&value, &owned_meta, 0);
    jet_rt_native_shared_drop(shared);
    assert(atomic_load(&drops) == before + 1);
    // The supplied glue, not meta.drop, owns final payload destruction.
    shared = fixture_shared_new((W)&value, &word_meta, (W)payload_drop);
    jet_rt_native_shared_drop(shared);
    assert(atomic_load(&drops) == before + 2);
#endif
    puts("shared concurrent edits/drop ok");
}

typedef struct {
    W shared;
    atomic_uint *arrived, *start;
} SharedCloneRace;
static void *shared_clone_racer(void *argument) {
    jet_rt_thread_attach();
    SharedCloneRace *race = argument;
    W shared = race->shared;
    atomic_fetch_add(race->arrived, 1);
    while (!atomic_load(race->start)) sched_yield();
    for (W i = 0; i < 100000; ++i) {
        W clone = jet_rt_native_shared_clone(shared);
        jet_rt_native_shared_drop(clone);
    }
    jet_rt_native_shared_drop(shared);
    jet_rt_thread_detach();
    return NULL;
}

static void shared_clone_drop_race(void) {
    W value = 17;
    unsigned before = atomic_load(&drops);
    W shared = fixture_shared_new((W)&value, &owned_meta, (W)payload_drop);
    atomic_uint arrived = 0, start = 0;
    SharedCloneRace owners[2] = {
        {jet_rt_native_shared_clone(shared), &arrived, &start},
        {jet_rt_native_shared_clone(shared), &arrived, &start}};
    pthread_t workers[2];
    for (size_t i = 0; i < 2; ++i)
        assert(!pthread_create(&workers[i], NULL, shared_clone_racer, &owners[i]));
    while (atomic_load(&arrived) != 2) sched_yield();
    // The root does not pin the cell through join: a worker owns last release.
    jet_rt_native_shared_drop(shared);
    atomic_store(&start, 1);
    for (size_t i = 0; i < 2; ++i) assert(!pthread_join(workers[i], NULL));
    assert(atomic_load(&drops) == before + 1);
    puts("shared two-thread 100k clone/drop exactly-once payload drop ok");
}

#ifdef JET_CHANNEL_INTERNAL
static W guard_drop_probe;
static void guard_payload_drop(W payload) {
    (void)payload;
    assert(guard_drop_probe);
    W *cell = (W *)guard_drop_probe;
    assert(cell[1] == 0 && cell[3] == 0 && cell[4] == 0 && cell[9] == 0);
    atomic_fetch_add_explicit(&drops, 1, memory_order_relaxed);
}
static const Meta guard_meta = {8,0,(W)guard_payload_drop,0,0,0,2,0,0};
static void shared_guard_lifetime(void) {
    W value = 19;
    unsigned before = atomic_load(&drops);
    W shared = fixture_shared_new((W)&value, &guard_meta, (W)guard_payload_drop);
    W first = jet_rt_native_shared_guard_read(shared);
    W second = jet_rt_native_shared_guard_read(shared);
    assert(first == shared && second == shared);
    assert(jet_rt_native_shared_read_guard_read(first) == jet_rt_native_shared_read_guard_deref(second));
    assert(*(W *)jet_rt_native_shared_read_guard_deref(first) == 19);
    jet_rt_native_shared_drop(shared);
    jet_rt_native_shared_read_guard_drop(first);
    assert(atomic_load(&drops) == before);
    guard_drop_probe = shared;
    jet_rt_native_shared_read_guard_drop(second);
    guard_drop_probe = 0;
    assert(atomic_load(&drops) == before + 1);
    shared = fixture_shared_new((W)&value, &guard_meta, (W)guard_payload_drop);
    W edit = jet_rt_native_shared_guard_edit(shared);
    assert(edit == shared && jet_rt_native_shared_guard_revision(edit) == 0);
    assert(jet_rt_native_shared_edit_guard_read(edit) == jet_rt_native_shared_edit_guard_edit(edit));
    jet_rt_native_shared_drop(shared);
    guard_drop_probe = shared;
    jet_rt_native_shared_edit_guard_drop(edit);
    guard_drop_probe = 0;
    assert(atomic_load(&drops) == before + 2);
    jet_rt_channel_fixture_shared_guard_stops(&word_meta);
    puts("shared guard retain/drop/read-only commit/overflow stop ok");
}
typedef struct {
    W shared;
    atomic_uint *arrived;
    atomic_uint *release;
    atomic_uint *stage;
    unsigned kind;
} Reader;

static uint64_t read_gate(W env, W input, W output) {
    Reader *r = (Reader *)env;
    atomic_fetch_add(r->arrived, 1);
    while (!atomic_load(r->release)) sched_yield();
    *(W *)output = *(W *)input;
    return 0;
}

static uint64_t ordered_writer(W env, W input, W output) {
    Reader *r = (Reader *)env;
    assert(atomic_load(r->release));
    assert(atomic_load(r->stage) == 0);
    *(W *)input += 1;
    atomic_store(r->stage, 1);
    *(W *)output = *(W *)input;
    return 0;
}

static uint64_t ordered_reader(W env, W input, W output) {
    Reader *r = (Reader *)env;
    assert(atomic_load(r->stage) == 1);
    atomic_store(r->stage, 2);
    *(W *)output = *(W *)input;
    return 0;
}

static void *reader(void *arg) {
    jet_rt_thread_attach();
    Reader *r = arg;
    W out = 0;
    if (r->kind == 1) {
        jet_rt_native_shared_edit(r->shared, (W)r, (W)ordered_writer, (W)&out);
    } else {
        W callback = r->kind == 2 ? (W)ordered_reader : (W)read_gate;
        jet_rt_native_shared_read(r->shared, (W)r, callback, (W)&out);
    }
    jet_rt_native_shared_drop(r->shared);
    jet_rt_thread_detach();
    return NULL;
}

static void *guard_reader(void *arg) {
    jet_rt_thread_attach();
    Reader *r = arg;
    W out = 0;
    if (r->kind == 1) {
        W guard = jet_rt_native_shared_guard_edit(r->shared);
        assert(jet_rt_native_shared_edit_guard_read(guard) == jet_rt_native_shared_edit_guard_edit(guard));
        ordered_writer((W)r, jet_rt_native_shared_edit_guard_edit(guard), (W)&out);
        jet_rt_native_shared_edit_guard_drop(guard);
    } else {
        W guard = jet_rt_native_shared_guard_read(r->shared);
        assert(jet_rt_native_shared_read_guard_read(guard) == jet_rt_native_shared_read_guard_deref(guard));
        if (r->kind == 2) ordered_reader((W)r, jet_rt_native_shared_read_guard_deref(guard), (W)&out);
        else read_gate((W)r, jet_rt_native_shared_read_guard_deref(guard), (W)&out);
        jet_rt_native_shared_read_guard_drop(guard);
    }
    jet_rt_native_shared_drop(r->shared);
    jet_rt_thread_detach();
    return NULL;
}

static void shared_readers(bool guards) {
    void *(*start)(void *) = guards ? guard_reader : reader;
    W value = 9, shared = fixture_shared_new((W)&value, &word_meta, 0);
    atomic_uint arrived = 0, release = 0, stage = 0;
    Reader args[4];
    pthread_t threads[4];
    for (size_t i = 0; i < 2; ++i) {
        args[i] = (Reader){jet_rt_native_shared_clone(shared),
                          &arrived, &release, &stage, 0};
        assert(!pthread_create(&threads[i], NULL, start, &args[i]));
    }
    uint64_t deadline = now_ns() + 5000000000ULL;
    while (atomic_load(&arrived) != 2) {
        assert(now_ns() < deadline);
        sched_yield();
    }
    // Both callbacks are inside canonical read permits at this barrier.
    args[2] = (Reader){jet_rt_native_shared_clone(shared),
                      &arrived, &release, &stage, 1};
    assert(!pthread_create(&threads[2], NULL, start, &args[2]));
    while (!jet_rt_channel_fixture_writers_waiting(shared)) {
        assert(now_ns() < deadline);
        sched_yield();
    }
    args[3] = (Reader){jet_rt_native_shared_clone(shared),
                      &arrived, &release, &stage, 2};
    assert(!pthread_create(&threads[3], NULL, start, &args[3]));
    atomic_store(&release, 1);
    for (size_t i = 0; i < 4; ++i) assert(!pthread_join(threads[i], NULL));
    assert(atomic_load(&stage) == 2);
    jet_rt_native_shared_drop(shared);
    puts(guards ? "shared guards concurrent readers/writer exclusion/priority ok" : "shared concurrent readers/writer priority ok");
}
#endif

int main(int argc,char **argv){jet_rt_thread_attach();if(argc>1&&!strcmp(argv[1],"perf")){ping(1000000);return 0;}if(argc>1&&!strcmp(argv[1],"select-closed")){W tx,rx;jet_rt_native_channel(0,0,&tx,&rx);jet_rt_native_sender_drop(tx);Header rs=words(&rx,1),ts=words(NULL,0);int64_t arm;W value;jet_rt_native_select_wait(&rs,&ts,0,&arm,&value);return 99;}if(argc>1&&!strcmp(argv[1],"select-empty")){Header rs=words(NULL,0),ts=words(NULL,0);int64_t arm;W value;jet_rt_native_select_wait(&rs,&ts,0,&arm,&value);return 99;}queue_tests();select_tests();shared_tests();
    shared_clone_drop_race();
#ifdef JET_CHANNEL_INTERNAL
    shared_readers(false);
    shared_readers(true);
    shared_guard_lifetime();
#endif
    ping(10000);jet_rt_thread_detach();return 0;}
