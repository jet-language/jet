// Link real composed Jet runtime exports with -Wl,--wrap=jet_rt_alloc
// -Wl,--wrap=jet_rt_free. Events.jet against the Rust meaning
// (ReactiveEventWatch.rs JetEventScope/JetSubscription/JetEvent): registration
// order, priority, once, cancellation, scope teardown, nested emit, and every
// provider allocation freed exactly once.
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { const char *data; int64_t length, capacity; } Text;
extern void jet_rt_alloc_init(void);
extern void *__real_jet_rt_alloc(int64_t, int64_t);
extern void __real_jet_rt_free(void *, int64_t, int64_t);
extern void jet_rt_string_drop(Text *);
extern int64_t *jet_event_policy_sync(void);
extern void jet_rt_handle_drop_jet_event_policy(int64_t *);
extern void *jet_event_with_policy(int64_t *);
extern void *jet_rt_handle_clone_jet_event(void *);
extern void jet_rt_handle_drop_jet_event(void *);
extern void *jet_event_listen(void *, void *, int64_t, int64_t, void *, void *, void *);
extern void *jet_event_emit(void *, void *);
extern int64_t jet_event_listener_count(void *);
extern Text *jet_event_trace(void *);
extern void *jet_event_scope_new(void);
extern void jet_event_scope_cancel(void *);
extern int64_t jet_event_scope_active_count(void *);
extern void *jet_rt_handle_clone_jet_event_scope(void *);
extern void jet_rt_handle_drop_jet_event_scope(void *);
extern void jet_subscription_unsubscribe(void *);
extern _Bool jet_subscription_active(void *);
extern void *jet_rt_handle_clone_jet_subscription(void *);
extern void jet_rt_handle_drop_jet_subscription(void *);
extern int64_t jet_event_trace_delivered(void *);
extern int64_t jet_event_trace_queued(void *);
extern int64_t jet_event_trace_dropped(void *);
extern Text *jet_event_trace_summary(void *);
extern void *jet_rt_handle_clone_jet_event_trace(void *);
extern void jet_rt_handle_drop_jet_event_trace(void *);

// Every live allocation, so each scenario proves it returns to its baseline.
static struct { void *pointer; int64_t size; } live[4096];
static size_t live_count, allocated;
void *__wrap_jet_rt_alloc(int64_t size, int64_t alignment) {
    void *pointer = __real_jet_rt_alloc(size, alignment);
    assert(live_count < sizeof live / sizeof live[0]);
    live[live_count].pointer = pointer;
    live[live_count++].size = size;
    ++allocated;
    return pointer;
}
void __wrap_jet_rt_free(void *pointer, int64_t size, int64_t alignment) {
    if (pointer != NULL) {
        size_t at = 0;
        while (at < live_count && live[at].pointer != pointer) ++at;
        if (at < live_count) {
            assert(live[at].size == size);
            live[at] = live[--live_count];
        }
    }
    __real_jet_rt_free(pointer, size, alignment);
}

// A handler closure as the lowering's lazy.callback adapter sees it.
typedef struct Handler Handler;
struct Handler {
    const char *name;
    char *log;
    int released;
    void (*body)(Handler *, int64_t);
    void *other;
    void *event;
};
static int64_t invoke(Handler *env, int64_t *slot, int64_t *scratch) {
    (void)scratch;
    assert(env->released == 0);
    char line[64];
    snprintf(line, sizeof line, "%s:%lld ", env->name, (long long)*slot);
    strcat(env->log, line);
    if (env->body) env->body(env, *slot);
    return 0;
}
static void release(Handler *env) { assert(++env->released == 1); }
static Handler handler(const char *name, char *log) { Handler h = {name, log, 0, NULL, NULL, NULL}; return h; }
static void *listen(void *event, void *scope, int64_t priority, int64_t once, Handler *h) {
    return jet_event_listen(event, scope, priority, once, h, (void *)invoke, (void *)release);
}
static int64_t emit(void *event, int64_t payload, int64_t expect_delivered) {
    void *trace = jet_event_emit(event, &payload);
    assert(trace != NULL);
    int64_t delivered = jet_event_trace_delivered(trace);
    assert(delivered == expect_delivered);
    assert(jet_event_trace_queued(trace) == 0 && jet_event_trace_dropped(trace) == 0);
    jet_rt_handle_drop_jet_event_trace(trace);
    return delivered;
}
static void text_is(Text *text, const char *expected) {
    assert(text->length == (int64_t)strlen(expected));
    assert(memcmp(text->data, expected, strlen(expected)) == 0);
    jet_rt_string_drop(text);
}
static void *new_event(void) {
    int64_t *policy = jet_event_policy_sync();
    assert(*policy == 0);
    void *event = jet_event_with_policy(policy);
    jet_rt_handle_drop_jet_event_policy(policy);
    return event;
}

// Examples/features/concurrency/event_policy.jet and callback_output.jet.
static void goldens(void) {
    void *events = new_event();
    assert(jet_event_listener_count(events) == 0);
    text_is(jet_event_trace(events), "listeners=0 queued=0 dropped=0");
    jet_rt_handle_drop_jet_event(events);

    char log[256] = "";
    void *app = jet_event_scope_new();
    void *saved = new_event();
    Handler seen = handler("seen", log);
    jet_rt_handle_drop_jet_subscription(listen(saved, app, 0, 0, &seen));
    void *trace = jet_event_emit(saved, &(int64_t){7});
    assert(strcmp(log, "seen:7 ") == 0);
    assert(jet_event_trace_delivered(trace) == 1);
    void *copy = jet_rt_handle_clone_jet_event_trace(trace);
    jet_rt_handle_drop_jet_event_trace(trace);
    text_is(jet_event_trace_summary(copy), "event delivered=1 queued=0 dropped=0");
    jet_rt_handle_drop_jet_event_trace(copy);
    jet_event_scope_cancel(app);
    assert(seen.released == 1);
    assert(jet_event_listener_count(saved) == 0);
    assert(jet_event_scope_active_count(app) == 0);
    jet_rt_handle_drop_jet_event_scope(app);
    jet_rt_handle_drop_jet_event(saved);
}

// Examples/features/ui/events.jet's sync Event lines: on + once, unsubscribe.
static void once_and_unsubscribe(void) {
    char log[256] = "";
    void *scope = jet_event_scope_new();
    void *clicked = new_event();
    Handler on = handler("clicked", log), once = handler("once", log);
    void *sub = listen(clicked, scope, 0, 0, &on);
    jet_rt_handle_drop_jet_subscription(listen(clicked, scope, 0, 1, &once));
    emit(clicked, 1, 2);
    assert(once.released == 1);
    emit(clicked, 2, 1);
    assert(jet_event_scope_active_count(scope) == 1);
    assert(jet_subscription_active(sub));
    jet_subscription_unsubscribe(sub);
    jet_subscription_unsubscribe(sub);
    assert(!jet_subscription_active(sub) && on.released == 1);
    emit(clicked, 3, 0);
    assert(jet_event_scope_active_count(scope) == 0);
    assert(strcmp(log, "clicked:1 once:1 clicked:2 ") == 0);
    jet_event_scope_cancel(scope);
    assert(jet_event_scope_active_count(scope) == 0);
    jet_rt_handle_drop_jet_subscription(sub);
    jet_rt_handle_drop_jet_event_scope(scope);
    jet_rt_handle_drop_jet_event(clicked);
}

// Priority first (highest), registration order among equal priorities.
static void priority_order(void) {
    char log[256] = "";
    void *scope = jet_event_scope_new();
    void *event = new_event();
    Handler a = handler("a", log), b = handler("b", log), c = handler("c", log), d = handler("d", log);
    void *subs[4] = {
        listen(event, scope, 0, 0, &a), listen(event, scope, 10, 0, &b),
        listen(event, scope, 0, 0, &c), listen(event, scope, 10, 0, &d),
    };
    assert(jet_event_listener_count(event) == 4);
    emit(event, 5, 4);
    assert(strcmp(log, "b:5 d:5 a:5 c:5 ") == 0);
    for (int i = 0; i < 4; ++i) jet_rt_handle_drop_jet_subscription(subs[i]);
    // Dropping a subscription handle keeps its listener (the scope tracks it).
    assert(jet_event_listener_count(event) == 4 && jet_event_scope_active_count(scope) == 4);
    jet_rt_handle_drop_jet_event_scope(scope);
    assert(a.released && b.released && c.released && d.released);
    assert(jet_event_listener_count(event) == 0);
    jet_rt_handle_drop_jet_event(event);
}

// Scope teardown: a clone keeps the scope alive; the last handle cancels.
static void scope_teardown(void) {
    char log[256] = "";
    void *scope = jet_event_scope_new();
    void *event = new_event();
    Handler h = handler("h", log);
    void *sub = listen(event, scope, 0, 0, &h);
    void *alias = jet_rt_handle_clone_jet_event_scope(scope);
    jet_rt_handle_drop_jet_event_scope(alias);
    assert(jet_subscription_active(sub) && h.released == 0);
    emit(event, 1, 1);
    jet_rt_handle_drop_jet_event_scope(scope);
    assert(!jet_subscription_active(sub) && h.released == 1);
    emit(event, 2, 0);
    jet_rt_handle_drop_jet_subscription(sub);
    jet_rt_handle_drop_jet_event(event);
    assert(strcmp(log, "h:1 ") == 0);
}

// A cancelled scope returns an inactive subscription and releases the handler.
static void cancelled_scope(void) {
    char log[64] = "";
    void *scope = jet_event_scope_new();
    jet_event_scope_cancel(scope);
    jet_event_scope_cancel(scope);
    void *event = new_event();
    Handler h = handler("late", log);
    void *sub = listen(event, scope, 0, 0, &h);
    assert(!jet_subscription_active(sub) && h.released == 1);
    assert(jet_event_listener_count(event) == 0 && jet_event_scope_active_count(scope) == 0);
    emit(event, 1, 0);
    jet_rt_handle_drop_jet_subscription(sub);
    jet_rt_handle_drop_jet_event_scope(scope);
    jet_rt_handle_drop_jet_event(event);
    assert(log[0] == 0);
}

// Mid-dispatch: a handler unsubscribing a later listener skips it; one
// unsubscribing itself keeps running (its closure is released after it returns).
static void unsubscribe_self(Handler *self, int64_t payload) { (void)payload; jet_subscription_unsubscribe(self->other); assert(self->released == 0); }
static void unsubscribe_other(Handler *self, int64_t payload) { (void)payload; jet_subscription_unsubscribe(self->other); }
static void mutation_during_dispatch(void) {
    char log[256] = "";
    void *scope = jet_event_scope_new();
    void *event = new_event();
    Handler first = handler("first", log), second = handler("second", log), third = handler("third", log);
    first.body = unsubscribe_self;
    second.body = unsubscribe_other;
    void *s1 = listen(event, scope, 0, 0, &first);
    void *s2 = listen(event, scope, 0, 0, &second);
    void *s3 = listen(event, scope, 0, 0, &third);
    first.other = s1;
    second.other = s3;
    emit(event, 1, 2);
    assert(first.released == 1 && third.released == 1 && second.released == 0);
    emit(event, 2, 1);
    assert(strcmp(log, "first:1 second:1 second:2 ") == 0);
    jet_rt_handle_drop_jet_subscription(s1);
    jet_rt_handle_drop_jet_subscription(s2);
    jet_rt_handle_drop_jet_subscription(s3);
    jet_rt_handle_drop_jet_event_scope(scope);
    jet_rt_handle_drop_jet_event(event);
}

// AllowDepthFirst: a nested emit dispatches immediately; a once listener is
// consumed before its handler runs, so re-emitting does not deliver it twice.
static void nested(Handler *self, int64_t payload) {
    if (payload < 3) { void *trace = jet_event_emit(self->event, &(int64_t){payload + 1}); jet_rt_handle_drop_jet_event_trace(trace); }
}
static void nested_emit(void) {
    char log[256] = "";
    void *scope = jet_event_scope_new();
    void *event = new_event();
    Handler once = handler("once", log), outer = handler("outer", log), tail = handler("tail", log);
    once.body = nested; once.event = event;
    outer.body = nested; outer.event = event;
    jet_rt_handle_drop_jet_subscription(listen(event, scope, 5, 1, &once));
    jet_rt_handle_drop_jet_subscription(listen(event, scope, 0, 0, &outer));
    jet_rt_handle_drop_jet_subscription(listen(event, scope, 0, 0, &tail));
    emit(event, 1, 3);
    assert(strcmp(log, "once:1 outer:2 outer:3 tail:3 tail:2 outer:1 outer:2 outer:3 tail:3 tail:2 tail:1 ") == 0);
    assert(once.released == 1 && jet_event_listener_count(event) == 2);
    jet_rt_handle_drop_jet_event_scope(scope);
    jet_rt_handle_drop_jet_event(event);
}

// The last Event handle releases its handlers; subscriptions outlive it.
static void event_drop_first(void) {
    char log[64] = "";
    void *scope = jet_event_scope_new();
    void *event = new_event();
    void *alias = jet_rt_handle_clone_jet_event(event);
    Handler h = handler("h", log);
    void *sub = listen(event, scope, 0, 0, &h);
    void *kept = jet_rt_handle_clone_jet_subscription(sub);
    jet_rt_handle_drop_jet_event(event);
    assert(h.released == 0 && jet_event_listener_count(alias) == 1);
    jet_rt_handle_drop_jet_event(alias);
    assert(h.released == 1 && jet_subscription_active(sub));
    jet_rt_handle_drop_jet_subscription(sub);
    jet_subscription_unsubscribe(kept);
    assert(!jet_subscription_active(kept) && jet_event_scope_active_count(scope) == 0);
    jet_rt_handle_drop_jet_subscription(kept);
    jet_rt_handle_drop_jet_event_scope(scope);
}

typedef void (*Scenario)(void);
int main(void) {
    jet_rt_alloc_init();
    Scenario scenarios[] = {goldens, once_and_unsubscribe, priority_order, scope_teardown, cancelled_scope, mutation_during_dispatch, nested_emit, event_drop_first};
    goldens(); // warm any runtime-lifetime allocation before baselines
    for (size_t i = 0; i < sizeof scenarios / sizeof scenarios[0]; ++i) {
        size_t before = live_count;
        scenarios[i]();
        assert(live_count == before);
    }
    assert(allocated > 0); // the provider's own allocations are intercepted
    puts("Event provider: order, priority, once, cancellation, teardown, nested emit, ownership OK");
    return 0;
}
