/* Native host proof for the Jet-as-guest embedding contract. */
#include "embedding.h"

#include <atomic>
#include <csignal>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <dlfcn.h>
#include <string>
#include <thread>
#include <vector>

#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>

using Tick = int64_t (*)(int64_t);
using Enabled = bool (*)(bool);
using Greet = JetText (*)(JetText);
using FreeText = void (*)(JetText);
using Reenter = int64_t (*)(int64_t);
using Panic = int64_t (*)(int64_t);

static Tick nested_tick = nullptr;

[[noreturn]] void fail(const char *message) {
    std::fprintf(stderr, "guest embedding: %s\n", message);
    std::exit(1);
}

/* Re-entry row: a #Import(c) host callback calls another live export. */
extern "C" int64_t host_reenter(int64_t value) {
    if (nested_tick == nullptr) {
        return -1;
    }
    return nested_tick(value) + 1000;
}

void signal_marker(int) {}

template <typename Function>
Function symbol(void *handle, const char *name) {
    void *address = dlsym(handle, name);
    if (address == nullptr) {
        fail(name);
    }
    return reinterpret_cast<Function>(address);
}

#if defined(__linux__)
std::size_t resident_bytes() {
    FILE *statm = std::fopen("/proc/self/statm", "r");
    if (statm == nullptr) {
        return 0;
    }
    unsigned long long pages = 0;
    unsigned long long resident = 0;
    const int scanned = std::fscanf(statm, "%llu %llu", &pages, &resident);
    std::fclose(statm);
    const long page_size = sysconf(_SC_PAGESIZE);
    if (scanned != 2 || page_size <= 0) {
        return 0;
    }
    return static_cast<std::size_t>(resident) * static_cast<std::size_t>(page_size);
}
#else
std::size_t resident_bytes() {
    return 0;
}
#endif

bool call_surface(Tick tick, Enabled enabled, Greet greet, FreeText free_text) {
    if (tick(tick(41)) != 43 || !enabled(true)) {
        return false;
    }

    const char *name = "Ada";
    JetText greeting = greet({reinterpret_cast<const uint8_t *>(name), 3});
    const bool valid = greeting.ptr != nullptr && greeting.len == 11 &&
        std::string(reinterpret_cast<const char *>(greeting.ptr), greeting.len) == "hello, Ada!";
    /* Allocator ownership row: every Text result is released by the Library. */
    free_text(greeting);
    return valid;
}

void call_on_threads(Tick tick, Enabled enabled, Greet greet, FreeText free_text) {
    std::atomic<bool> failed{false};
    std::vector<std::thread> workers;
    /* Thread entry row: two host threads make 1,000 calls each. */
    for (int index = 0; index < 2; ++index) {
        workers.emplace_back([&] {
            for (int iteration = 0; iteration < 1000; ++iteration) {
                if (!call_surface(tick, enabled, greet, free_text)) {
                    failed.store(true);
                }
            }
        });
    }
    for (auto &worker : workers) {
        worker.join();
    }
    if (failed.load()) {
        fail("threaded calls failed");
    }
}

void run_cycles(const char *path) {
    /* Signals row: the host's handler must survive every Library cycle. */
    auto previous_signal = std::signal(SIGUSR1, signal_marker);
    if (previous_signal == SIG_ERR) {
        fail("cannot install signal marker");
    }

    std::size_t first_resident = 0;
    for (int cycle = 0; cycle < 3; ++cycle) {
        /* Initialization row: dlopen is the only admission step. */
        void *handle = dlopen(path, RTLD_NOW | RTLD_LOCAL);
        if (handle == nullptr) {
            fail("dlopen failed");
        }
        if (dlsym(handle, "jet_init") != nullptr || dlsym(handle, "jet_shutdown") != nullptr) {
            fail("unexpected init/shutdown symbol");
        }

        auto tick = symbol<Tick>(handle, "on_tick");
        auto enabled = symbol<Enabled>(handle, "is_enabled");
        auto greet = symbol<Greet>(handle, "greet");
        auto reenter = symbol<Reenter>(handle, "reenter");
        auto free_text = symbol<FreeText>(handle, "jet_text_free");
        nested_tick = tick;
        /* Re-entry row: the imported host callback enters this export again. */
        if (reenter(41) != 1042 || !call_surface(tick, enabled, greet, free_text)) {
            fail("direct or reentrant calls failed");
        }
        call_on_threads(tick, enabled, greet, free_text);
        nested_tick = nullptr;

        /* Shutdown row: all calls and Text releases finish before dlclose. */
        if (dlclose(handle) != 0) {
            fail("dlclose failed");
        }
        const std::size_t resident = resident_bytes();
        if (cycle == 0) {
            first_resident = resident;
        } else if (first_resident != 0 && resident > first_resident + 32U * 1024U * 1024U) {
            fail("resident memory grew across load/unload cycles");
        }
        auto current_signal = std::signal(SIGUSR1, previous_signal);
        if (current_signal != signal_marker) {
            fail("Library changed host signal handling");
        }
        previous_signal = std::signal(SIGUSR1, signal_marker);
        if (previous_signal == SIG_ERR) {
            fail("cannot restore signal marker");
        }
    }

    std::signal(SIGUSR1, previous_signal);
}

void run_isolated_panic(const char *path) {
    /* Panic row: isolate the terminal E3001/status-70 boundary in a child. */
    const pid_t child = fork();
    if (child < 0) {
        fail("fork failed");
    }
    if (child == 0) {
        void *handle = dlopen(path, RTLD_NOW | RTLD_LOCAL);
        if (handle == nullptr) {
            _exit(96);
        }
        auto panic = symbol<Panic>(handle, "panic_now");
        panic(7);
        _exit(95);
    }

    int status = 0;
    if (waitpid(child, &status, 0) != child || !WIFEXITED(status) || WEXITSTATUS(status) != 70) {
        fail("panic child did not return status 70");
    }

    /* Parent continuation row: the host remains alive after child isolation. */
    void *handle = dlopen(path, RTLD_NOW | RTLD_LOCAL);
    if (handle == nullptr) {
        fail("parent continuation dlopen failed");
    }
    auto tick = symbol<Tick>(handle, "on_tick");
    if (tick(41) != 42 || dlclose(handle) != 0) {
        fail("parent continuation call failed");
    }
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fail("usage: foreign <library>");
    }
    run_cycles(argv[1]);
    run_isolated_panic(argv[1]);
    std::puts("embedding-ok");
    return 0;
}
