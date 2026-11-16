/* concurrency-service: a line-oriented TCP service whose `batch N` fans values 1..N out to four
   pthread workers through a mutex/condvar queue and sums their squares. */
#include <arpa/inet.h>
#include <netinet/in.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

#define WORKERS 4
#define MAX_BATCH 32

typedef struct {
    pthread_mutex_t lock;
    pthread_cond_t ready, finished;
    int values[MAX_BATCH];
    int head, tail;        /* queued values are values[head..tail) */
    int pending;           /* jobs of the current batch not yet summed */
    long total;
    int stopping;
} Queue;

static Queue queue = {PTHREAD_MUTEX_INITIALIZER, PTHREAD_COND_INITIALIZER, PTHREAD_COND_INITIALIZER};

static void *worker(void *unused) {
    (void)unused;
    pthread_mutex_lock(&queue.lock);
    for (;;) {
        while (queue.head == queue.tail && !queue.stopping) pthread_cond_wait(&queue.ready, &queue.lock);
        if (queue.head == queue.tail) break;
        int value = queue.values[queue.head++];
        pthread_mutex_unlock(&queue.lock);
        nanosleep(&(struct timespec){0, 1000000}, NULL); /* the 1ms of work every rail's job does */
        long square = (long)value * value;
        pthread_mutex_lock(&queue.lock);
        queue.total += square;
        if (--queue.pending == 0) pthread_cond_signal(&queue.finished);
    }
    pthread_mutex_unlock(&queue.lock);
    return NULL;
}

static long run_batch(int count) {
    pthread_mutex_lock(&queue.lock);
    queue.head = 0;
    queue.tail = 0;
    queue.total = 0;
    queue.pending = count;
    for (int value = 1; value <= count; value++) queue.values[queue.tail++] = value;
    pthread_cond_broadcast(&queue.ready);
    while (queue.pending > 0) pthread_cond_wait(&queue.finished, &queue.lock);
    long total = queue.total;
    pthread_mutex_unlock(&queue.lock);
    return total;
}

/* Answers one connection's command line; returns 1 when it was `shutdown`. */
static int serve(int conn) {
    char line[128];
    size_t len = 0;
    ssize_t n;
    while (len < sizeof line - 1 && (n = read(conn, line + len, sizeof line - 1 - len)) > 0) {
        len += (size_t)n;
        if (memchr(line, '\n', len)) break;
    }
    line[len] = '\0';
    char *end = line + strcspn(line, "\r\n");
    *end = '\0';
    while (end > line && (end[-1] == ' ' || end[-1] == '\t')) *--end = '\0';
    char *command = line + strspn(line, " \t");

    char response[64] = "error\n";
    int stop = 0, count;
    char extra;
    if (strcmp(command, "ready") == 0) {
        strcpy(response, "ready\n");
    } else if (strcmp(command, "shutdown") == 0) {
        strcpy(response, "bye\n");
        stop = 1;
    } else if (sscanf(command, "batch %d%c", &count, &extra) == 1 && count > 0 && count <= MAX_BATCH) {
        snprintf(response, sizeof response, "batch %d total %ld\n", count, run_batch(count));
    }
    if (write(conn, response, strlen(response)) < 0) perror("write");
    close(conn);
    return stop;
}

int main(int argc, char **argv) {
    if (argc < 2) {
        fputs("usage: concurrency-service PORT\n", stderr);
        return 2;
    }
    int listener = socket(AF_INET, SOCK_STREAM, 0);
    setsockopt(listener, SOL_SOCKET, SO_REUSEADDR, &(int){1}, sizeof(int));
    struct sockaddr_in address = {.sin_family = AF_INET, .sin_port = htons((unsigned short)atoi(argv[1])),
                                  .sin_addr.s_addr = htonl(INADDR_LOOPBACK)};
    if (bind(listener, (struct sockaddr *)&address, sizeof address) != 0 || listen(listener, 64) != 0) {
        perror("listen");
        return 1;
    }
    pthread_t workers[WORKERS];
    for (int i = 0; i < WORKERS; i++) pthread_create(&workers[i], NULL, worker, NULL);
    for (int stopping = 0; !stopping;) {
        int conn = accept(listener, NULL, NULL);
        if (conn < 0) break;
        stopping = serve(conn);
    }
    close(listener);
    pthread_mutex_lock(&queue.lock);
    queue.stopping = 1;
    pthread_cond_broadcast(&queue.ready);
    pthread_mutex_unlock(&queue.lock);
    for (int i = 0; i < WORKERS; i++) pthread_join(workers[i], NULL);
    return 0;
}
