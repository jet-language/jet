// Allocation ledger-zero: pending RtL8a6 ProgramAllocator facts SHA.
// Pipe24 ownership/position witnesses against real Jet allocation and OS fds.
// No Rust layout, fake allocator, synthetic stop or fixed host line iterator.
#define _POSIX_C_SOURCE 200809L
#include <assert.h>
#include <errno.h>
#include <signal.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

typedef intptr_t Word;
typedef struct { Word stride, clone, drop, show, equal, compare, kind, child0, child1; } Meta;
typedef struct { Word payload, next, release, clone; } Adapter;
typedef struct { Word lock, fd, closed, path, buffer, capacity, filled, position, refs; } FD;
extern Word jet_fd_retain_raw(Word);
extern void jet_fd_drop_raw(Word);
extern Word jet_fd_write_raw(Word, const void *, Word, Word *, bool *);
extern Word jet_fd_seek_raw(Word, Word, Word, Word *, bool *);
extern Word jet_fd_adopt_raw(Word, Word, bool);
extern Word jet_process_pipe_take_raw(Word);
extern void jet_process_pipe_close_raw(Word);
extern Word jet_process_stream_line_bytes_raw(Word, Word, Word, Word);
extern Word jet_rt_handle_clone_process_stdout_stream(Word);
extern void jet_rt_handle_drop_process_stdout_stream(Word);
extern Word jet_rt_handle_clone_process_stderr_stream(Word);
extern void jet_rt_handle_drop_process_stderr_stream(Word);
extern void jet_rt_handle_drop_io_error(Word);
extern Word jet_rt_native_iter_process_lines(Word, Word, Word);
extern Word jet_rt_native_iter_next(Word);
extern void jet_rt_native_iter_drop(Word);
extern void jet_rt_free(Word, Word, Word);
extern bool jet_rt_comptime_begin(uint64_t, uint64_t);
extern void jet_rt_comptime_work(uint64_t);
extern void jet_rt_comptime_end(void);
extern Word jet_rt_session_propagate(void);
static int releases;
static int transferred_fd;
static Word pipe_of(const unsigned char *bytes, size_t length) {
    int ends[2];
    assert(pipe(ends) == 0);
    assert(write(ends[1], bytes, length) == (ssize_t)length);
    assert(close(ends[1]) == 0);
    transferred_fd = ends[0];
    return jet_process_pipe_take_raw(jet_fd_adopt_raw(ends[0], 0, false));
}
static void line(Word held, const unsigned char *expected, size_t length) {
    Word data = 0, len = 0, error = 0;
    assert(jet_process_stream_line_bytes_raw(held, (Word)&data, (Word)&len, (Word)&error) == 1);
    assert(!error && len == (Word)length);
    assert(length ? data != 0 : data == 0);
    if (length) assert(memcmp((void *)data, expected, length) == 0);
    jet_rt_free(data, len, 1);
}
static void closed(int fd) {
    errno = 0;
    char byte;
    assert(read(fd, &byte, 1) == -1 && errno == EBADF);
}
static Word release(Word held) {
    ++releases;
    jet_rt_handle_drop_process_stdout_stream(held);
    return 0;
}
static Word clone(Word held) { return jet_rt_handle_clone_process_stdout_stream(held); }
static Word stop_next(Word held, Word output) {
    // Read real shared buffered bytes first; fuel then stops the callback.
    Word data = 0, len = 0, error = 0;
    (void)output;
    assert(jet_process_stream_line_bytes_raw(held, (Word)&data, (Word)&len, (Word)&error) == 1);
    assert(len == 2 && !error);
    jet_rt_free(data, len, 1);
    jet_rt_comptime_work(UINT64_C(1000001));
    assert(jet_rt_session_propagate());
    return -1; // No moved output slot was initialized.
}
static Word reentrant_pipe;
static void reenter(int signal_number) {
    Word data = 0, len = 0, error = 0;
    (void)signal_number;
    // The outer kernel read still owns RefCell's mutable borrow.
    (void)jet_process_stream_line_bytes_raw(reentrant_pipe, (Word)&data, (Word)&len, (Word)&error);
    _Exit(99); // A returned borrow would violate the real RefCell contract.
}
static void borrow_stop(void) {
    int capture[2];
    assert(pipe(capture) == 0);
    pid_t child = fork();
    assert(child >= 0);
    if (!child) {
        assert(dup2(capture[1], STDERR_FILENO) == STDERR_FILENO);
        close(capture[0]); close(capture[1]);
        int ends[2]; assert(pipe(ends) == 0);
        // Keep the write half open: the outer read blocks, then SIGALRM
        // re-enters the same holder instead of forging its borrow word.
        reentrant_pipe = jet_process_pipe_take_raw(jet_fd_adopt_raw(ends[0], 0, false));
        struct sigaction action = {0};
        action.sa_handler = reenter;
        sigemptyset(&action.sa_mask);
        assert(sigaction(SIGALRM, &action, NULL) == 0);
        alarm(1);
        Word data = 0, len = 0, error = 0;
        (void)jet_process_stream_line_bytes_raw(reentrant_pipe, (Word)&data, (Word)&len, (Word)&error);
        _Exit(98);
    }
    close(capture[1]);
    char report[4096]; size_t used = 0;
    while (used + 1 < sizeof(report)) {
        ssize_t got = read(capture[0], report + used, sizeof(report) - used - 1);
        if (got < 0 && errno == EINTR) continue;
        assert(got >= 0);
        if (!got) break;
        used += (size_t)got;
    }
    report[used] = 0;
    close(capture[0]);
    int status; assert(waitpid(child, &status, 0) == child);
    assert(WIFEXITED(status) && WEXITSTATUS(status) == 70);
    assert(strstr(report, "RefCell already borrowed"));
}
static void writer_take(void) {
    FILE *file=tmpfile(); assert(file);
    int number=dup(fileno(file)); assert(number>=0);
    Word fd=jet_fd_adopt_raw(number,0,true);
    Word alias=jet_fd_retain_raw(fd), facts[4]; bool zero=false;
    assert(jet_fd_write_raw(fd,"a\nb\n",4,facts,&zero)==4 && !facts[0]);
    assert(((FD *)fd)->filled==4);
    Word holder=jet_process_pipe_take_raw(fd);
    line(holder,NULL,0); // flush then kernel EOF; never expose write bytes
    assert(((FD *)alias)->capacity==8192 && ((FD *)alias)->filled==0);
    assert(lseek(number,0,SEEK_CUR)==4);
    assert(jet_fd_seek_raw(alias,0,SEEK_SET,facts,&zero)==0 && !facts[0]);
    line(holder,(const unsigned char *)"a\n",2);
    // The other line is now in shared refill storage. Writing through a
    // retained alias must hit the kernel, not append to/alter that storage.
    Word filled=((FD *)alias)->filled, position=((FD *)alias)->position;
    assert(jet_fd_write_raw(alias,"c\n",2,facts,&zero)==2 && !facts[0]);
    assert(((FD *)alias)->filled==filled && ((FD *)alias)->position==position);
    assert(lseek(number,0,SEEK_CUR)==6);
    line(holder,(const unsigned char *)"b\n",2);
    line(holder,NULL,0);
    jet_fd_drop_raw(alias); jet_rt_handle_drop_process_stdout_stream(holder);
    closed(number); assert(!fclose(file));
    int ends[2]; assert(!pipe(ends));
    fd=jet_fd_adopt_raw(ends[0],0,false); alias=jet_fd_retain_raw(fd);
    assert(write(ends[1],"x\ny\n",4)==4 && !close(ends[1]));
    holder=jet_process_pipe_take_raw(fd);
    line(holder,(const unsigned char *)"x\n",2);
    assert(jet_fd_write_raw(alias,"z",1,facts,&zero)==-1 && facts[0]==1 && facts[1]==EBADF);
    line(holder,(const unsigned char *)"y\n",2);
    jet_fd_drop_raw(alias); jet_rt_handle_drop_process_stdout_stream(holder);
}
int main(void) {
    writer_take();
    Word (*clones[])(Word) = {jet_rt_handle_clone_process_stdout_stream, jet_rt_handle_clone_process_stderr_stream};
    void (*drops[])(Word) = {jet_rt_handle_drop_process_stdout_stream, jet_rt_handle_drop_process_stderr_stream};
    for (size_t family = 0; family < 2; ++family) {
        const unsigned char bytes[] = {'a', '\r', '\n', 'b', '\n', 0xff, '\r'};
        Word holder = pipe_of(bytes, sizeof(bytes));
        int fd = transferred_fd;
        Word first = clones[family](holder), second = clones[family](holder);
        assert(first == holder && second == holder);
        assert(*(Word *)holder == 3); // Canonical Pipe24 refs word.
        // One refill contains all three lines. Interleaving distinct clones
        // must share buffered position, not only the kernel fd offset.
        line(first, bytes, 3);
        line(second, bytes + 3, 2);
        drops[family](holder);
        assert(*(Word *)first == 2);
        line(first, bytes + 5, 2); // Invalid UTF8/bare CR retained for Core.
        line(second, NULL, 0);
        drops[family](first);
        assert(*(Word *)second == 1);
        line(second, NULL, 0);
        drops[family](second);
        closed(fd);
        holder = pipe_of((const unsigned char *)"x\ny\n", 4);
        fd = transferred_fd;
        first = clones[family](holder);
        jet_process_pipe_close_raw(first);
        line(holder, NULL, 0); // Shared None is visible through the other clone.
        jet_process_pipe_close_raw(holder); // Idempotent Option::take.
        drops[family](first); drops[family](holder);
        closed(fd);
        drops[family](0);
    }
    Word holder = jet_process_pipe_take_raw(0);
    line(holder, NULL, 0);
    jet_rt_handle_drop_process_stdout_stream(holder);
    holder = pipe_of((const unsigned char *)"a\nb\n", 4);
    int fd = transferred_fd;
    assert(jet_rt_comptime_begin(UINT64_C(1000000), UINT64_C(1000)));
    Adapter source = {holder, (Word)stop_next, (Word)release, (Word)clone};
    Meta meta = {8, 0, 0, 0, 0, 0, 0, 0, 0};
    Word iter = jet_rt_native_iter_process_lines((Word)&source, 1, (Word)&meta);
    assert(!jet_rt_native_iter_next(iter));
    jet_rt_native_iter_drop(iter);
    assert(releases == 1);
    jet_rt_comptime_end();
    closed(fd);
    // Real syscall error: byte/error ownership is exclusive at the result seam.
    holder = pipe_of((const unsigned char *)"x\n", 2);
    fd = transferred_fd;
    assert(close(fd) == 0);
    Word data = 0, len = 0, error = 0;
    assert(jet_process_stream_line_bytes_raw(holder, (Word)&data, (Word)&len, (Word)&error) == 0);
    assert(!data && !len && error);
    jet_rt_handle_drop_io_error(error);
    jet_rt_handle_drop_process_stdout_stream(holder);
    borrow_stop();
    puts("process pipe: shared refill/clone/close, raw bytes, early-stop release and RefCell panic");
    return 0;
}
