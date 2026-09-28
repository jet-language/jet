/* Native C++ host proof for `jet build --lib library.jet`. */
#include "loadable.h"

#include <atomic>
#include <csignal>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <dlfcn.h>
#include <string>
#include <thread>
#include <vector>

using Tick = int64_t (*)(int64_t);
using Enabled = bool (*)(bool);
using Greet = JetText (*)(JetText);
using FreeText = void (*)(JetText);
using Panic = int64_t (*)(int64_t);

[[noreturn]] void fail(const char *message) {
    std::fprintf(stderr, "foreign C++: %s\n", message);
    std::exit(1);
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

bool call_surface(Tick tick, Enabled enabled, Greet greet, FreeText free_text) {
    if (tick(tick(41)) != 43 || !enabled(true)) {
        return false;
    }

    const char *name = "Ada";
    JetText greeting = greet({reinterpret_cast<const uint8_t *>(name), 3});
    const bool valid = greeting.ptr != nullptr && greeting.len == 11 &&
        std::string(reinterpret_cast<const char *>(greeting.ptr), greeting.len) == "hello, Ada!";
    free_text(greeting);
    return valid;
}

void call_on_threads(Tick tick, Enabled enabled, Greet greet, FreeText free_text) {
    std::atomic<bool> failed{false};
    std::vector<std::thread> workers;
    for (int index = 0; index < 4; ++index) {
        workers.emplace_back([&] {
            for (int iteration = 0; iteration < 8; ++iteration) {
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
    auto previous_signal = std::signal(SIGUSR1, signal_marker);
    if (previous_signal == SIG_ERR) {
        fail("cannot install signal marker");
    }

    for (int cycle = 0; cycle < 3; ++cycle) {
        void *handle = dlopen(path, RTLD_NOW | RTLD_LOCAL);
        if (handle == nullptr) {
            fail("dlopen failed");
        }

        // D-EMBED2=C: native Library has no process-global init/shutdown API.
        if (dlsym(handle, "jet_init") != nullptr || dlsym(handle, "jet_shutdown") != nullptr) {
            fail("unexpected init/shutdown symbol");
        }

        auto tick = symbol<Tick>(handle, "on_tick");
        auto enabled = symbol<Enabled>(handle, "is_enabled");
        auto greet = symbol<Greet>(handle, "greet");
        auto free_text = symbol<FreeText>(handle, "jet_text_free");
        if (!call_surface(tick, enabled, greet, free_text)) {
            fail("direct calls failed");
        }
        call_on_threads(tick, enabled, greet, free_text);

        if (dlclose(handle) != 0) {
            fail("dlclose failed");
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
    std::puts("cpp-ok");
}

int main(int argc, char **argv) {
    if (argc == 2) {
        run_cycles(argv[1]);
        return 0;
    }
    if (argc == 3 && std::strcmp(argv[1], "--panic") == 0) {
        void *handle = dlopen(argv[2], RTLD_NOW | RTLD_LOCAL);
        if (handle == nullptr) {
            fail("dlopen failed");
        }
        symbol<Panic>(handle, "panic_now")(0);
        return 2;
    }
    fail("usage: foreign <library> | foreign --panic <library>");
}
