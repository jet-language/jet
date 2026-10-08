// Only native Jet hooks/Sync/TLS; pthreads and subprocesses are test transport.
#define _POSIX_C_SOURCE 200809L
#include <assert.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <stdatomic.h>
#include <string.h>
#include <pthread.h>
#include <unistd.h>
#include <sys/wait.h>
typedef struct { const unsigned char *data; int64_t length, capacity; } Text;
typedef struct { int64_t refs; int64_t (*invoke)(void *, const Text *); int64_t (*release)(void *); int64_t answer; } Closure;
_Static_assert(sizeof(Text) == 24 && sizeof(Closure) == 32, "native transport carrier geometry");
extern void jet_rt_vault_test_hooks_defaults_fixture(const Text *, const Text *, const Text *);
extern void jet_rt_alloc_init(void);
extern void jet_rt_thread_init(void);
extern void jet_rt_thread_attach(void);
extern void jet_rt_thread_detach(void);
extern void *jet_rt_alloc(int64_t, int64_t);
extern void jet_rt_free(void *, int64_t, int64_t);
extern void jet_rt_vault_test_hooks_init(int64_t (*)(void *, void *));
extern int64_t jet_rt_vault_test_hooks_destroy(void *, void *);
extern void jet_rt_vault_set_test_authorizer(Closure *);
extern void jet_rt_vault_clear_test_authorizer(void);
extern int64_t jet_rt_vault_test_authorize(const Text *);
extern void jet_rt_vault_set_test_trust_path(const Text *);
extern const Text *jet_rt_vault_test_trust_path(void);
extern void jet_rt_vault_set_test_fault(const Text *);
extern _Bool jet_rt_vault_test_fault(const Text *);
extern void *jet_rt_vault_test_barrier_new(int64_t);
extern void jet_rt_vault_test_barrier_wait(void *);
extern void jet_rt_vault_test_barrier_drop(void *);
extern void jet_rt_vault_set_test_backup_barrier(const Text *, void *, void *);
extern void jet_rt_vault_test_pause_after_backup(const Text *);
static atomic_uint destroyed, released, completed;
static _Thread_local unsigned this_thread_destroyed;
static Text text(const char *s) { return (Text){(const unsigned char *)s, (int64_t)strlen(s), 0}; }
static int64_t destroy(void *closure, void *state) {
    assert(++this_thread_destroyed == 1);
    atomic_fetch_add(&destroyed, 1);
    return jet_rt_vault_test_hooks_destroy(closure, state);
}
static int64_t alternate_destroy(void *closure, void *state) { return jet_rt_vault_test_hooks_destroy(closure, state); }
static int64_t invoke(void *raw, const Text *preview) {
    Closure *closure = raw;
    const char *expected = "generate SigningKey release";
    assert(preview->length == (int64_t)strlen(expected));
    assert(memcmp(preview->data, expected, strlen(expected)) == 0);
    if (closure->answer == -2) jet_rt_vault_clear_test_authorizer();
    return closure->answer;
}
static int64_t release(void *raw) {
    atomic_fetch_add(&released, 1);
    jet_rt_free(raw, (int64_t)sizeof(Closure), 8);
    return 0;
}
static Closure *authorizer(int64_t answer) {
    Closure *closure = jet_rt_alloc((int64_t)sizeof(Closure), 8);
    *closure = (Closure){1, invoke, release, answer};
    return closure;
}
static void defaults(void) {
    Text preview = text("generate SigningKey release"), fault = text("cancel-before-install"), root = text("/vault/default");
    jet_rt_vault_test_hooks_defaults_fixture(&preview, &fault, &root);
}
static void *thread_hooks(void *raw) {
    const char *name = raw;
    jet_rt_thread_attach();
    defaults();
    Text local = text(name), preview = text("generate SigningKey release"), main_fault = text("cancel-before-install");
    jet_rt_vault_set_test_fault(&local);
    jet_rt_vault_set_test_trust_path(&local);
    assert(jet_rt_vault_test_fault(&local));
    assert(!jet_rt_vault_test_fault(&main_fault));
    const Text *trust = jet_rt_vault_test_trust_path();
    assert(trust->length == local.length && memcmp(trust->data, local.data, (size_t)local.length) == 0);
    jet_rt_vault_set_test_authorizer(authorizer(256));
    assert(jet_rt_vault_test_authorize(&preview) == 0); // low-byte Bool, not whole-word truthiness
    jet_rt_thread_detach();
    assert(this_thread_destroyed == 1);
    return NULL;
}
static void *backup_writer(void *unused) {
    (void)unused;
    jet_rt_thread_attach();
    Text root = text("/vault/selected"), fault = text("writer");
    jet_rt_vault_set_test_fault(&fault); // witnesses writer TLS destructor, too
    for (unsigned round = 0; round < 8; ++round) {
        jet_rt_vault_test_pause_after_backup(&root);
        atomic_store(&completed, round + 1);
    }
    jet_rt_thread_detach();
    assert(this_thread_destroyed == 1);
    return NULL;
}
static void negative(const char *mode, int expected_exit, const char *expected_stderr, _Bool exact) {
    int pipes[2]; assert(pipe(pipes) == 0);
    pid_t child = fork(); assert(child >= 0);
    if (child == 0) {
        assert(dup2(pipes[1], STDERR_FILENO) == STDERR_FILENO);
        close(pipes[0]); close(pipes[1]);
        execl("/proc/self/exe", "VaultHooksWitness", mode, (char *)NULL);
        _exit(127);
    }
    close(pipes[1]);
    char message[4096]; size_t used = 0;
    for (;;) {
        ssize_t n = read(pipes[0], message + used, sizeof(message) - 1 - used);
        assert(n >= 0);
        if (n == 0) break;
        used += (size_t)n; assert(used < sizeof(message) - 1);
    }
    message[used] = 0; close(pipes[0]);
    int status; assert(waitpid(child, &status, 0) == child);
    assert(WIFEXITED(status) && WEXITSTATUS(status) == expected_exit);
    assert(exact ? strcmp(message, expected_stderr) == 0 : strstr(message, expected_stderr) != NULL);
}
int main(int argc, char **argv) {
    jet_rt_alloc_init(); jet_rt_thread_init();
    defaults();
    if (argc == 2 && strcmp(argv[1], "null") == 0) { jet_rt_vault_test_hooks_init(NULL); return 1; }
    jet_rt_vault_test_hooks_init(destroy);
    jet_rt_vault_test_hooks_init(destroy); // same pointer is a no-op
    if (argc == 2 && strcmp(argv[1], "different") == 0) { jet_rt_vault_test_hooks_init(alternate_destroy); return 1; }
    Text preview = text("generate SigningKey release");
    if (argc == 2 && strcmp(argv[1], "borrow") == 0) {
        jet_rt_vault_set_test_authorizer(authorizer(-2));
        (void)jet_rt_vault_test_authorize(&preview); return 1;
    }
    defaults();
    negative("borrow", 101, "RefCell already borrowed\n", 1);
    negative("null", 70, "vault test hooks initializer received a null destructor", 0);
    negative("different", 70, "vault test hooks initializer changed its destructor", 0);
    Text main_fault = text("cancel-before-install"), other_fault = text("cancel-before-install-extra"), trust_path = text("/vault/main-trust");
    jet_rt_vault_set_test_fault(&main_fault);
    assert(jet_rt_vault_test_fault(&main_fault) && !jet_rt_vault_test_fault(&other_fault));
    jet_rt_vault_set_test_trust_path(&trust_path);
    jet_rt_vault_set_test_trust_path(jet_rt_vault_test_trust_path()); // borrowed alias survives replacement
    jet_rt_vault_set_test_authorizer(authorizer(257));
    assert(jet_rt_vault_test_authorize(&preview) == 1);
    jet_rt_vault_set_test_authorizer(authorizer(0));
    assert(atomic_load(&released) == 1 && jet_rt_vault_test_authorize(&preview) == 0);
    jet_rt_vault_clear_test_authorizer();
    assert(atomic_load(&released) == 2 && jet_rt_vault_test_authorize(&preview) == -1);
    pthread_t workers[3];
    const char *names[] = {"fault-one", "fault-two", "fault-three"};
    for (unsigned i = 0; i < 3; ++i) assert(pthread_create(&workers[i], NULL, thread_hooks, (void *)names[i]) == 0);
    for (unsigned i = 0; i < 3; ++i) assert(pthread_join(workers[i], NULL) == 0);
    assert(atomic_load(&destroyed) == 3 && atomic_load(&released) == 5);
    assert(jet_rt_vault_test_fault(&main_fault));
    void *entered = jet_rt_vault_test_barrier_new(2), *resume = jet_rt_vault_test_barrier_new(2);
    Text root = text("/vault/selected"), wrong = text("/vault/other");
    jet_rt_vault_set_test_backup_barrier(&root, entered, resume);
    jet_rt_vault_test_pause_after_backup(&wrong); // must not enter either barrier
    pthread_t writer; assert(pthread_create(&writer, NULL, backup_writer, NULL) == 0);
    for (unsigned round = 0; round < 8; ++round) {
        jet_rt_vault_test_barrier_wait(entered);
        assert(atomic_load(&completed) == round); // second phase still blocked
        if (round == 7) jet_rt_vault_set_test_backup_barrier(NULL, NULL, NULL); // held snapshot remains valid
        jet_rt_vault_test_barrier_wait(resume);
    }
    assert(pthread_join(writer, NULL) == 0 && atomic_load(&completed) == 8);
    jet_rt_vault_test_barrier_drop(entered); jet_rt_vault_test_barrier_drop(resume);
    for (int64_t count = 0; count <= 1; ++count) {
        void *single = jet_rt_vault_test_barrier_new(count);
        for (unsigned round = 0; round < 8; ++round) jet_rt_vault_test_barrier_wait(single);
        jet_rt_vault_test_barrier_drop(single);
    }
    assert(atomic_load(&destroyed) == 4);
    jet_rt_vault_set_test_trust_path(NULL); jet_rt_vault_set_test_fault(NULL);
    defaults();
    jet_rt_thread_detach();
    assert(this_thread_destroyed == 1 && atomic_load(&destroyed) == 5 && atomic_load(&released) == 5);
    puts("vault hooks witness OK (TLS ownership, exact stops, 8 root-filtered two-phase barriers)");
    return 0;
}
