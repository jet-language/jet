#define _GNU_SOURCE
#include <poll.h>
#include <signal.h>
#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <unistd.h>

extern char **environ;

/* Start argv with stdin/stdout redirected to the given fds (-1 keeps the parent's); closes `close_fd` in the child. */
static pid_t start(char *const argv[], int in, int out, int close_fd) {
    posix_spawn_file_actions_t actions;
    posix_spawn_file_actions_init(&actions);
    if (in >= 0) posix_spawn_file_actions_adddup2(&actions, in, STDIN_FILENO);
    if (out >= 0) posix_spawn_file_actions_adddup2(&actions, out, STDOUT_FILENO);
    if (close_fd >= 0) posix_spawn_file_actions_addclose(&actions, close_fd);
    pid_t pid;
    int error = posix_spawnp(&pid, argv[0], &actions, NULL, argv, environ);
    posix_spawn_file_actions_destroy(&actions);
    if (error != 0) {
        fprintf(stderr, "cannot start %s: %s\n", argv[0], strerror(error));
        exit(1);
    }
    return pid;
}

/* All of fd's bytes, NUL-terminated, with trailing whitespace trimmed. */
static char *read_trimmed(int fd) {
    size_t len = 0, cap = 256;
    char *text = malloc(cap);
    ssize_t n;
    while ((n = read(fd, text + len, cap - len - 1)) > 0) {
        len += (size_t)n;
        if (cap - len < 2) text = realloc(text, cap *= 2);
    }
    while (len > 0 && (text[len - 1] == '\n' || text[len - 1] == ' ' || text[len - 1] == '\r')) len--;
    text[len] = '\0';
    return text;
}

static int wait_exit(pid_t pid) {
    int status;
    waitpid(pid, &status, 0);
    return WIFEXITED(status) ? WEXITSTATUS(status) : -1;
}

int main(void) {
    /* calc: capture a child's stdout. */
    int pipe_fds[2];
    pipe(pipe_fds);
    pid_t calc = start((char *[]){"python3", "-c", "print(6*7)", NULL}, -1, pipe_fds[1], pipe_fds[0]);
    close(pipe_fds[1]);
    char *answer = read_trimmed(pipe_fds[0]);
    close(pipe_fds[0]);
    wait_exit(calc);
    printf("calc %s\n", answer);

    /* sorted: python's lines piped through sort. */
    int source_pipe[2], sorted_pipe[2];
    pipe(source_pipe);
    pipe(sorted_pipe);
    pid_t source = start((char *[]){"python3", "-c", "print('b');print('a');print('c')", NULL}, -1, source_pipe[1], source_pipe[0]);
    close(source_pipe[1]);
    pid_t sorter = start((char *[]){"sort", NULL}, source_pipe[0], sorted_pipe[1], sorted_pipe[0]);
    close(source_pipe[0]);
    close(sorted_pipe[1]);
    char *sorted = read_trimmed(sorted_pipe[0]);
    close(sorted_pipe[0]);
    wait_exit(source);
    wait_exit(sorter);
    for (char *p = sorted; *p; p++)
        if (*p == '\n') *p = ',';
    printf("sorted %s\n", sorted);

    /* slow: a child that outlives a 300ms deadline is killed. */
    pid_t slow = start((char *[]){"python3", "-c", "import time;time.sleep(5)", NULL}, -1, -1, -1);
    int pidfd = (int)syscall(SYS_pidfd_open, slow, 0);
    struct pollfd exited = {.fd = pidfd, .events = POLLIN};
    if (pidfd >= 0 && poll(&exited, 1, 300) == 0) {
        kill(slow, SIGKILL);
        puts("slow timeout");
    }
    wait_exit(slow);
    if (pidfd >= 0) close(pidfd);

    /* exit: a checked run observes the child's exit code. */
    pid_t checked = start((char *[]){"python3", "-c", "import sys;sys.exit(3)", NULL}, -1, -1, -1);
    int code = wait_exit(checked);
    if (code != 0) printf("exit %d\n", code);
    return 0;
}
