#include <cstdlib>
#include <cstdint>
#include <thread>

#ifdef _WIN32
#include <windows.h>
using Module = HMODULE;

static Module open_module(const char *path) {
    return LoadLibraryA(path);
}

static void *lookup(Module module) {
    return reinterpret_cast<void *>(GetProcAddress(module, "on_tick"));
}

static bool close_module(Module module) {
    return FreeLibrary(module) != 0;
}
#else
#include <dlfcn.h>
using Module = void *;

static Module open_module(const char *path) {
    return dlopen(path, RTLD_NOW);
}

static void *lookup(Module module) {
    return dlsym(module, "on_tick");
}

static bool close_module(Module module) {
    return dlclose(module) == 0;
}
#endif

using Tick = std::int64_t (*)(std::int64_t);

int main(int argc, char **argv) {
    if (argc != 2) {
        return 2;
    }
    for (int round = 0; round != 3; ++round) {
        Module module = open_module(argv[1]);
        if (module == nullptr) {
            return 3;
        }
        auto tick = reinterpret_cast<Tick>(lookup(module));
        if (tick == nullptr || tick(41) != 42) {
            close_module(module);
            return 4;
        }
        bool worker_ok = false;
        std::thread worker([&] { worker_ok = tick(41) == 42; });
        worker.join();
        if (!worker_ok || !close_module(module)) {
            return 5;
        }
    }
    return 0;
}
