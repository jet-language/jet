#define _GNU_SOURCE
#include <errno.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/un.h>
#include <unistd.h>

static void write_file(const char *path, const char *text) {
    FILE *file = fopen(path, "w");
    if (!file) exit(20);
    fputs(text, file);
    fclose(file);
}

static int run_nix(int argc, char **argv) {
    const char *root = getenv("JET_TEST_TOOL_ROOT");
    const char *missing = getenv("JET_TEST_MISSING_LOCKED_INPUT");
    char dir[PATH_MAX], path[PATH_MAX];
    int disk = 0;
    int offline = 0;
    for (int i = 1; i < argc; i++) if (!strcmp(argv[i], "--offline")) offline = 1;
    if (getenv("JET_TEST_REQUIRE_OFFLINE") && !offline) return 24;
    for (int i = 1; i < argc; i++) {
        if (!strcmp(argv[i], "metadata")) {
            if (getenv("JET_TEST_REQUIRE_COMPOSED_METADATA") && strcmp(argv[argc - 1], "path:.")) return 25;
            if (missing) {
                fprintf(stderr, "cannot fetch input '%s' because offline mode is enabled\n", missing);
                return 1;
            }
            puts("{}");
            return 0;
        }
    }
    for (int i = 1; i < argc; i++) if (strstr(argv[i], "#disk")) disk = 1;
    if (disk) {
        snprintf(dir, sizeof(dir), "%s/disk", root);
        mkdir(dir, 0700);
        snprintf(path, sizeof(path), "%s/system.qcow2", dir);
        write_file(path, "qcow2");
    } else {
        snprintf(dir, sizeof(dir), "%s/firmware", root);
        mkdir(dir, 0700);
        snprintf(path, sizeof(path), "%s/FV", dir);
        mkdir(path, 0700);
        snprintf(path, sizeof(path), "%s/FV/OVMF_CODE.fd", dir);
        write_file(path, "code");
        snprintf(path, sizeof(path), "%s/FV/OVMF_VARS.fd", dir);
        write_file(path, "vars");
    }
    printf("%s/%s\n", root, disk ? "disk" : "firmware");
    return 0;
}

static int run_qemu(int argc, char **argv) {
    const char *serial = NULL, *qmp = NULL;
    for (int i = 1; i + 1 < argc; i++) {
        if (!strcmp(argv[i], "-serial") && !strncmp(argv[i + 1], "file:", 5)) serial = argv[i + 1] + 5;
        if (!strcmp(argv[i], "-qmp") && !strncmp(argv[i + 1], "unix:", 5)) qmp = argv[i + 1] + 5;
    }
    if (!serial || !qmp) return 21;
    char socket_path[PATH_MAX];
    snprintf(socket_path, sizeof(socket_path), "%s", qmp);
    char *comma = strchr(socket_path, ',');
    if (comma) *comma = '\0';

    int server = socket(AF_UNIX, SOCK_STREAM, 0);
    struct sockaddr_un address = { .sun_family = AF_UNIX };
    snprintf(address.sun_path, sizeof(address.sun_path), "%s", socket_path);
    unlink(socket_path);
    if (bind(server, (struct sockaddr *)&address, sizeof(address)) || listen(server, 1)) return 22;

    const char *collision = getenv("JET_TEST_COLLIDE_PUBLISH");
    if (collision) {
        char path[PATH_MAX];
        snprintf(path, sizeof(path), "%s/.nixos-comparison-publish-halcyon-gnome-%d", collision, getppid());
        mkdir(path, 0700);
    }
    write_file(serial, "NIXOS_COMPARISON_PROOF:{\"host\":\"halcyon-gnome\",\"os_release\":\"NixOS\",\"prompt\":\"NixOS comparison $ \",\"banner\":\"NixOS comparison guest\",\"proof\":\"live-desktop\"}\n");

    int client = accept(server, NULL, NULL);
    FILE *stream = fdopen(client, "r+");
    fputs("{\"QMP\":{}}\n", stream);
    fflush(stream);
    char line[PATH_MAX * 2];
    while (fgets(line, sizeof(line), stream)) {
        char *filename = strstr(line, "\"filename\":\"");
        if (filename) {
            filename += strlen("\"filename\":\"");
            char *end = strchr(filename, '"');
            if (end) {
                *end = '\0';
                write_file(filename, "png");
            }
        }
        fputs("{\"return\":{}}\n", stream);
        fflush(stream);
        if (strstr(line, "system_powerdown")) break;
    }
    fclose(stream);
    close(server);
    unlink(socket_path);
    return 0;
}

int main(int argc, char **argv) {
    const char *name = strrchr(argv[0], '/');
    name = name ? name + 1 : argv[0];
    if (!strcmp(name, "nix")) return run_nix(argc, argv);
    if (!strcmp(name, "qemu-system-x86_64")) return run_qemu(argc, argv);
    return 0;
}
