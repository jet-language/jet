// Link against the COMPLETE composed Jet runtime object and canonical Int/E4.
// Fixtures cover every HostFiles export, kernel errno facts and FD72 ownership.
#define _GNU_SOURCE
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <signal.h>
#include <stdatomic.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/file.h>
#include <sys/stat.h>
#include <sys/xattr.h>
#include <time.h>
#include <unistd.h>
enum { PF_READ=1, PF_WRITE=2, PF_CREATE=4, PF_TRUNCATE=8, PF_APPEND=16,
       PF_EXCLUSIVE=32, PF_DIRECTORY=64, PF_NOFOLLOW=128 };
typedef int64_t W;
typedef struct { unsigned char *data; W len, cap; } Text;
typedef struct { W lock, fd, closed; Text *path; void *buffer; W cap, filled, pos, refs; } FD;
_Static_assert(sizeof(FD)==72 && sizeof(Text)==24, "host carrier layout");
extern FD *jet_fd_open_raw(Text *, W, W, W *);
extern FD *jet_fd_open_at_raw(FD *, Text *, W, W, W *);
extern FD *jet_fd_adopt_raw(W, Text *, bool);
extern FD *jet_fd_adopt_unbuffered_raw(W, Text *);
extern W jet_fd_number_raw(FD *);
extern Text *jet_fd_path_raw(FD *);
extern W jet_fd_read_raw(FD *, void *, W, W *);
extern W jet_fd_write_raw(FD *, const void *, W, W *, bool *);
extern W jet_fd_write_unbuffered_raw(FD *, const void *, W, W *);
extern void jet_fd_flush_raw(FD *, W *, bool *);
extern void jet_fd_sync_raw(FD *, bool, W *);
extern W jet_fd_seek_raw(FD *, W, W, W *, bool *);
extern void jet_fd_stat_raw(FD *, W *, W *);
extern void jet_fd_stat_at_raw(FD *, Text *, W *, W *);
extern void jet_fd_created_raw(FD *, W *, W *);
extern void jet_fd_chmod_raw(FD *, W, W *);
extern void jet_fd_lock_raw(FD *, W, W *);
extern void jet_fd_close_raw(FD *, W *, bool *);
extern FD *jet_fd_retain_raw(FD *);
extern void jet_fd_drop_raw(FD *);
extern FD *jet_fd_dup_raw(FD *, W *);
extern W jet_fd_directory_raw(FD *, void *, W, W *);
extern void jet_fd_link_at_raw(FD *, FD *, Text *, W *);
extern void jet_fd_truncate_raw(FD *, W, W *);
extern void jet_fd_mkdir_at_raw(FD *, Text *, W, W *);
extern void jet_fd_unlink_at_raw(FD *, Text *, bool, W *);
extern W jet_fd_readlink_at_raw(FD *, Text *, void *, W, W *);
extern void jet_fd_rename_at_raw(FD *, Text *, FD *, Text *, W, W *);
extern void jet_fs_stat_raw(Text *, bool, W *, W *);
extern void jet_fs_created_raw(Text *, bool, W *, W *);
extern void jet_fs_mkdir_raw(Text *, W, W *);
extern void jet_fs_unlink_raw(Text *, bool, W *);
extern void jet_fs_link_raw(Text *, Text *, bool, W *);
extern void jet_fs_rename_raw(Text *, Text *, W *);
extern W jet_fs_readlink_raw(Text *, void *, W, W *);
extern W jet_fs_xattr_get_raw(Text *, Text *, void *, W, W *);
extern void jet_rt_string_drop(Text *);
extern void jet_rt_host_program_start(void);
static bool success(W *error) { return !error[0] && !error[1] && !error[2] && !error[3]; }
static bool posix(W *error,W code) { return error[0]==1 && error[1]==code && error[2]==0 && error[3]==0; }
static Text text(char *s) { return (Text){(unsigned char *)s,(W)strlen(s),0}; }
typedef struct { FD *fd; W error[4],result; unsigned char byte; atomic_bool done; } ReadOnce;
static void interrupted(int signal) { (void)signal; }
static void *read_once(void *pointer) { ReadOnce *attempt=pointer;attempt->result=jet_fd_read_raw(attempt->fd,&attempt->byte,1,attempt->error);atomic_store(&attempt->done,true);return NULL; }
static void closed(FD *fd) {
    W error[4]={-1,-1,-1,-1}, facts[16]={0}; bool z=true; unsigned char byte;
    assert(jet_fd_number_raw(fd)==-1);
    assert(jet_fd_read_raw(fd,&byte,1,error)==-1 && posix(error, EBADF));
    assert(jet_fd_write_raw(fd,"x",1,error,&z)==-1 && posix(error, EBADF) && !z);
    assert(jet_fd_write_unbuffered_raw(fd,"x",1,error)==-1 && posix(error, EBADF));
    jet_fd_flush_raw(fd,error,&z); assert(posix(error, EBADF) && !z);
    jet_fd_sync_raw(fd,false,error); assert(posix(error, EBADF));
    assert(jet_fd_seek_raw(fd,0,SEEK_CUR,error,&z)==-1 && posix(error, EBADF) && !z);
    jet_fd_stat_raw(fd,facts,error); assert(posix(error, EBADF));
    jet_fd_created_raw(fd,facts,error); assert(posix(error, EBADF) && facts[0]==0);
    jet_fd_chmod_raw(fd,0600,error); assert(posix(error, EBADF));
    jet_fd_lock_raw(fd,LOCK_UN,error); assert(posix(error, EBADF));
    jet_fd_truncate_raw(fd,0,error); assert(posix(error, EBADF));
    assert(!jet_fd_dup_raw(fd,error) && posix(error, EBADF));
    assert(jet_fd_directory_raw(fd,&byte,1,error)==-1 && posix(error, EBADF));
    Text name=text("missing");
    assert(!jet_fd_open_at_raw(fd,&name,PF_READ,0,error) && posix(error, EBADF));
    jet_fd_mkdir_at_raw(fd,&name,0700,error); assert(posix(error, EBADF));
    jet_fd_unlink_at_raw(fd,&name,false,error); assert(posix(error, EBADF));
    assert(jet_fd_readlink_at_raw(fd,&name,&byte,1,error)==-1 && posix(error, EBADF));
    jet_fd_link_at_raw(fd,fd,&name,error); assert(posix(error, EBADF));
    jet_fd_rename_at_raw(fd,&name,fd,&name,0,error); assert(posix(error, EBADF));
    jet_fd_close_raw(fd,error,&z); assert(success(error) && !z); // idempotent
}
static void writer_reads(Text *path) {
    W error[4], facts[16]; bool zero; char byte=0x5a;
    FD *fd=jet_fd_open_raw(path,PF_READ|PF_WRITE|PF_CREATE|PF_TRUNCATE,0600,error);
    assert(fd && success(error) && fd->cap<0); // actual O_RDWR
    assert((fcntl((int)jet_fd_number_raw(fd),F_GETFL)&O_ACCMODE)==O_RDWR);
    assert(jet_fd_write_raw(fd,"a\n",2,error,&zero)==2 && success(error) && fd->filled==2);
    // Pending writes advance the shared kernel offset before read: EOF, not
    // "a" from the private writer buffer; the caller byte stays untouched.
    assert(jet_fd_read_raw(fd,&byte,1,error)==0 && success(error) && byte==0x5a);
    assert(fd->filled==0 && fd->cap<0);
    jet_fd_stat_raw(fd,facts,error); assert(success(error) && facts[7]==2);
    assert(jet_fd_seek_raw(fd,0,SEEK_SET,error,&zero)==0 && success(error));
    assert(jet_fd_read_raw(fd,&byte,1,error)==1 && success(error) && byte=='a');
    assert(jet_fd_seek_raw(fd,0,SEEK_END,error,&zero)==2 && success(error));
    assert(jet_fd_write_raw(fd,"b",1,error,&zero)==1 && success(error) && fd->filled==1);
    FD *duplicate=jet_fd_dup_raw(fd,error);
    assert(duplicate && success(error) && fd->filled==0 && duplicate->filled==0);
    struct stat native; assert(!fstat((int)jet_fd_number_raw(duplicate),&native) && native.st_size==3);
    assert(lseek((int)jet_fd_number_raw(duplicate),0,SEEK_CUR)==3);
    jet_fd_drop_raw(duplicate); jet_fd_drop_raw(fd);
    fd=jet_fd_open_raw(path,PF_WRITE|PF_TRUNCATE,0600,error);
    assert(fd && success(error));
    assert((fcntl((int)jet_fd_number_raw(fd),F_GETFL)&O_ACCMODE)==O_WRONLY);
    assert(jet_fd_write_raw(fd,"x",1,error,&zero)==1 && success(error) && fd->filled==1);
    byte=0x5a;
    assert(jet_fd_read_raw(fd,&byte,1,error)==-1 && posix(error,EBADF) && byte==0x5a);
    assert(fd->filled==0); // flush succeeded before the kernel rejected read
    assert(!fstat((int)jet_fd_number_raw(fd),&native) && native.st_size==1);
    jet_fd_drop_raw(fd);
    assert(!unlink((char *)path->data));
}
int main(void) {
    jet_rt_host_program_start();
    char root[]="/tmp/jet-files-XXXXXX"; assert(mkdtemp(root));
    char path[256], second[256], link[256], sub[256];
    snprintf(path,sizeof path,"%s/file",root);
    snprintf(second,sizeof second,"%s/second",root);
    snprintf(link,sizeof link,"%s/link",root);
    snprintf(sub,sizeof sub,"%s/sub",root);
    Text p=text(path), q=text(second), l=text(link), d=text(root), s=text(sub);
    W error[4]={-1,-1,-1,-1}, facts[16]; bool z=true; char bytes[16384];
    writer_reads(&q);
    FD *writer=jet_fd_open_raw(&p,PF_WRITE|PF_CREATE|PF_TRUNCATE,0600,error);
    assert(writer && success(error) && writer->refs==1 && writer->cap==-8192);
    assert(fcntl((int)jet_fd_number_raw(writer),F_GETFD)&FD_CLOEXEC);
    Text *shown=jet_fd_path_raw(writer); assert(shown->len==p.len && !memcmp(shown->data,p.data,p.len)); jet_rt_string_drop(shown);
    assert(jet_fd_write_raw(writer,"a\r\nb\0c",6,error,&z)==6 && success(error) && !z);
    jet_fd_stat_raw(writer,facts,error); assert(success(error) && facts[7]==0); // buffered
    jet_fd_flush_raw(writer,error,&z); assert(success(error) && !z);
    jet_fd_sync_raw(writer,false,error); assert(success(error));
    jet_fd_sync_raw(writer,true,error); assert(success(error));
    jet_fd_chmod_raw(writer,0640,error); assert(success(error));
    jet_fd_stat_raw(writer,facts,error); assert(success(error) && facts[7]==6 && (facts[2]&0777)==0640);
    struct stat expected; assert(!stat(path,&expected));
    assert((uint64_t)facts[0]==expected.st_dev && (uint64_t)facts[1]==expected.st_ino && facts[3]==(W)expected.st_nlink);
    assert(facts[4]==expected.st_uid && facts[5]==expected.st_gid && facts[12]==expected.st_ctim.tv_sec && facts[13]==expected.st_ctim.tv_nsec);
    assert(facts[14]==expected.st_blocks && facts[15]==expected.st_blksize);
    jet_fd_created_raw(writer,facts,error); assert(success(error) || posix(error, ENOSYS) || posix(error, EOPNOTSUPP)); assert(facts[0]==0 || facts[0]==1);
    jet_fd_lock_raw(writer,LOCK_EX|LOCK_NB,error); assert(success(error));
    jet_fd_lock_raw(writer,LOCK_UN,error); assert(success(error));
    jet_fd_truncate_raw(writer,5,error); assert(success(error));
    FD *alias=jet_fd_retain_raw(writer); assert(alias==writer && writer->refs==2);
    int native=(int)jet_fd_number_raw(writer);
    jet_fd_close_raw(writer,error,&z); assert(success(error) && !z); closed(alias);
    errno=0;assert(fcntl(native,F_GETFD)==-1 && errno==EBADF);
    jet_fd_drop_raw(writer); jet_fd_drop_raw(alias);
    FD *reader=jet_fd_open_raw(&p,PF_READ,0,error); assert(reader && success(error));
    assert(jet_fd_read_raw(reader,bytes,2,error)==2 && success(error) && !memcmp(bytes,"a\r",2));
    assert(jet_fd_seek_raw(reader,0,SEEK_CUR,error,&z)==2 && success(error) && !z);
    assert(jet_fd_read_raw(reader,bytes,3,error)==3 && !memcmp(bytes,"\nb\0",3));
    assert(jet_fd_read_raw(reader,bytes,1,error)==0 && success(error));
    assert(jet_fd_seek_raw(reader,0,SEEK_SET,error,&z)==0 && success(error));
    FD *duplicate=jet_fd_dup_raw(reader,error); assert(duplicate && duplicate!=reader && success(error));
    jet_fd_drop_raw(duplicate);
    FD *directory=jet_fd_open_raw(&d,PF_READ|PF_DIRECTORY,0,error); assert(directory && success(error));
    Text member=text("file"), hard=text("hard"), dir=text("dir"), renamed=text("renamed");
    FD *at=jet_fd_open_at_raw(directory,&member,PF_READ|PF_NOFOLLOW,0,error); assert(at && success(error)); jet_fd_drop_raw(at);
    jet_fd_stat_at_raw(directory,&member,facts,error); assert(success(error));
    assert((uint64_t)facts[1]==expected.st_ino && facts[7]==5);
    Text empty=text(""), absent=text("absent");
    jet_fd_stat_at_raw(directory,&empty,facts,error); assert(success(error) && (facts[2]&S_IFMT)==S_IFDIR);
    jet_fd_stat_at_raw(directory,&absent,facts,error); assert(posix(error,ENOENT));
    assert(jet_fd_directory_raw(directory,bytes,sizeof bytes,error)>0 && success(error));
    // Compare the exact held-inode syscall, not a broad allowed-error set.
    // link(2): no CAP_DAC_READ_SEARCH => ENOENT. FileHandles.rs does not try
    // /proc/self/fd + AT_SYMLINK_FOLLOW, so the native leaf must not either.
    int reference_link=linkat((int)jet_fd_number_raw(reader),"",
                             (int)jet_fd_number_raw(directory),"hard",AT_EMPTY_PATH);
    int link_errno=errno;
    if (!reference_link) assert(!unlinkat((int)jet_fd_number_raw(directory),"hard",0));
    jet_fd_link_at_raw(reader,directory,&hard,error);
    assert(reference_link ? posix(error,link_errno) : success(error));
    if (!reference_link) { jet_fd_unlink_at_raw(directory,&hard,false,error); assert(success(error)); }
    jet_fd_mkdir_at_raw(directory,&dir,0700,error); assert(success(error));
    jet_fd_rename_at_raw(directory,&dir,directory,&renamed,1,error); assert(success(error));
    jet_fd_unlink_at_raw(directory,&renamed,true,error); assert(success(error));
    jet_fs_link_raw(&p,&l,true,error); assert(success(error));
    assert(jet_fs_readlink_raw(&l,bytes,sizeof bytes,error)==p.len && success(error) && !memcmp(bytes,p.data,p.len));
    Text link_member=text("link"); assert(jet_fd_readlink_at_raw(directory,&link_member,bytes,sizeof bytes,error)==p.len && success(error));
    jet_fs_stat_raw(&l,false,facts,error); assert(success(error) && (facts[2]&S_IFMT)==S_IFLNK);
    assert(!lstat(link,&expected));
    jet_fd_stat_at_raw(directory,&link_member,facts,error); assert(success(error));
    assert((facts[2]&S_IFMT)==S_IFLNK && (uint64_t)facts[1]==expected.st_ino && facts[7]==expected.st_size);
    // Real lgetxattr is also the Rust FileHandles reference primitive.
    Text attribute=text("user.jet-host-fixture");
    ssize_t reference=lgetxattr(path,"user.jet-host-fixture",NULL,0);
    int reference_errno=errno;
    W actual=jet_fs_xattr_get_raw(&p,&attribute,NULL,0,error);
    assert(reference<0 && actual==-1 && posix(error,reference_errno));
    int set_result=lsetxattr(path,"user.jet-host-fixture","x\0y",3,0);
    if (!set_result) {
        assert(jet_fs_xattr_get_raw(&p,&attribute,NULL,0,error)==3 && success(error));
        assert(jet_fs_xattr_get_raw(&p,&attribute,bytes,2,error)==-1 && posix(error,ERANGE));
        assert(jet_fs_xattr_get_raw(&p,&attribute,bytes,sizeof bytes,error)==3 && success(error) && !memcmp(bytes,"x\0y",3));
        assert(jet_fs_xattr_get_raw(&l,&attribute,bytes,sizeof bytes,error)==-1 && posix(error,ENODATA));
        assert(!lsetxattr(path,"user.jet-host-fixture","",0,0));
        assert(jet_fs_xattr_get_raw(&p,&attribute,NULL,0,error)==0 && success(error));
    } else {
        assert(errno==ENOTSUP);
        assert(jet_fs_xattr_get_raw(&p,&attribute,NULL,0,error)==-1 && (posix(error,ENOTSUP) || posix(error,ENODATA)));
    }
    jet_fs_created_raw(&p,true,facts,error); assert(success(error) || posix(error, ENOSYS) || posix(error, EOPNOTSUPP));
    jet_fs_unlink_raw(&l,false,error); assert(success(error));
    jet_fs_link_raw(&p,&q,false,error); assert(success(error));
    jet_fs_rename_raw(&q,&l,error); assert(success(error));
    jet_fs_unlink_raw(&l,false,error); assert(success(error));
    jet_fs_mkdir_raw(&s,0700,error); assert(success(error));
    jet_fs_unlink_raw(&s,true,error); assert(success(error));
    jet_fd_drop_raw(reader);
    jet_fd_close_raw(directory,error,&z); assert(success(error) && !z);
    jet_fd_stat_at_raw(directory,&member,facts,error); assert(posix(error,EBADF));
    jet_fd_drop_raw(directory);
    FD *bulk=jet_fd_open_raw(&q,PF_WRITE|PF_CREATE|PF_TRUNCATE,0600,error);assert(bulk&&success(error));
    assert(jet_fd_write_raw(bulk,NULL,0,error,&z)==0 && success(error) && !z && !bulk->buffer);
    memset(bytes,'z',sizeof bytes);assert(jet_fd_write_raw(bulk,bytes,sizeof bytes,error,&z)==(W)sizeof bytes&&success(error)&&!bulk->buffer);jet_fd_drop_raw(bulk);
    bulk=jet_fd_open_raw(&q,PF_READ,0,error);assert(bulk&&success(error));
    memset(bytes,0,sizeof bytes);assert(jet_fd_read_raw(bulk,bytes,sizeof bytes,error)==(W)sizeof bytes&&success(error)&&!bulk->buffer);
    for(size_t i=0;i<sizeof bytes;i++)assert(bytes[i]=='z');
    jet_fd_drop_raw(bulk);jet_fs_unlink_raw(&q,false,error);assert(success(error));
    // A real non-restarting signal must expose EINTR without a hidden retry.
    int interrupted_pipe[2];assert(!pipe(interrupted_pipe));
    ReadOnce attempt={.fd=jet_fd_adopt_raw(interrupted_pipe[0],NULL,false),.byte=0xa5,.done=false};
    struct sigaction action={0};action.sa_handler=interrupted;sigemptyset(&action.sa_mask);assert(!sigaction(SIGUSR1,&action,NULL));
    pthread_t thread;assert(!pthread_create(&thread,NULL,read_once,&attempt));
    struct timespec pause={0,1000000};
    for(int i=0;i<1000&&!atomic_load(&attempt.done);i++){int signal_result=pthread_kill(thread,SIGUSR1);assert(!signal_result||signal_result==ESRCH);nanosleep(&pause,NULL);}
    assert(atomic_load(&attempt.done)&&!pthread_join(thread,NULL));
    assert(attempt.result==-1&&posix(attempt.error,EINTR)&&attempt.byte==0xa5&&!attempt.fd->buffer);
    assert(!close(interrupted_pipe[1]));assert(jet_fd_read_raw(attempt.fd,bytes,1,error)==0&&success(error));jet_fd_drop_raw(attempt.fd);
    int pipes[2]; assert(!pipe(pipes));
    FD *r=jet_fd_adopt_raw(pipes[0],NULL,false), *w=jet_fd_adopt_unbuffered_raw(pipes[1],NULL);
    assert(jet_fd_write_unbuffered_raw(w,"bin\0",4,error)==4 && success(error));
    assert(jet_fd_read_raw(r,bytes,4,error)==4 && success(error) && !memcmp(bytes,"bin\0",4));
    jet_fd_drop_raw(r);
    assert(jet_fd_write_unbuffered_raw(w,"x",1,error)==-1 && posix(error,EPIPE));
    jet_fd_drop_raw(w);
    jet_fs_unlink_raw(&p,false,error); assert(success(error));
    assert(!jet_fd_open_raw(&p,PF_READ,0,error) && posix(error, ENOENT));
    jet_fs_stat_raw(&p,true,facts,error); assert(posix(error, ENOENT));
    jet_fs_rename_raw(&p,&q,error); assert(posix(error, ENOENT));
    jet_fs_link_raw(&p,&q,false,error); assert(posix(error, ENOENT));
    assert(jet_fs_readlink_raw(&p,bytes,1,error)==-1 && posix(error, ENOENT));
    jet_fs_unlink_raw(&p,false,error); assert(posix(error, ENOENT));
    jet_fs_unlink_raw(&d,true,error); assert(success(error));
    puts("files raw: ok");
}
