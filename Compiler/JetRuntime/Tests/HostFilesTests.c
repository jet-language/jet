// Link against the COMPLETE composed Jet runtime object and canonical Int/E4.
// Fixtures cover every HostFiles export, kernel errno facts and FD72 ownership.
#define _GNU_SOURCE
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/file.h>
#include <sys/stat.h>
#include <unistd.h>
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
extern W jet_fd_read_unbuffered_raw(FD *, void *, W, W *);
extern W jet_fd_write_unbuffered_raw(FD *, const void *, W, W *);
extern void jet_fd_flush_raw(FD *, W *, bool *);
extern void jet_fd_sync_raw(FD *, bool, W *);
extern W jet_fd_seek_raw(FD *, W, W, W *, bool *);
extern void jet_fd_stat_raw(FD *, W *, W *);
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
extern void jet_rt_string_drop(Text *);
static Text text(char *s) { return (Text){(unsigned char *)s,(W)strlen(s),0}; }
static void closed(FD *fd) {
    W e=-1, facts[16]={0}; bool z=true; unsigned char byte;
    assert(jet_fd_number_raw(fd)==-1);
    assert(jet_fd_read_raw(fd,&byte,1,&e)==-1 && e==EBADF);
    assert(jet_fd_write_raw(fd,"x",1,&e,&z)==-1 && e==EBADF && !z);
    assert(jet_fd_read_unbuffered_raw(fd,&byte,1,&e)==-1 && e==EBADF);
    assert(jet_fd_write_unbuffered_raw(fd,"x",1,&e)==-1 && e==EBADF);
    jet_fd_flush_raw(fd,&e,&z); assert(e==EBADF && !z);
    jet_fd_sync_raw(fd,false,&e); assert(e==EBADF);
    assert(jet_fd_seek_raw(fd,0,SEEK_CUR,&e,&z)==-1 && e==EBADF && !z);
    jet_fd_stat_raw(fd,facts,&e); assert(e==EBADF);
    jet_fd_created_raw(fd,facts,&e); assert(e==EBADF && facts[0]==0);
    jet_fd_chmod_raw(fd,0600,&e); assert(e==EBADF);
    jet_fd_lock_raw(fd,LOCK_UN,&e); assert(e==EBADF);
    jet_fd_truncate_raw(fd,0,&e); assert(e==EBADF);
    assert(!jet_fd_dup_raw(fd,&e) && e==EBADF);
    assert(jet_fd_directory_raw(fd,&byte,1,&e)==-1 && e==EBADF);
    Text name=text("missing");
    assert(!jet_fd_open_at_raw(fd,&name,O_RDONLY,0,&e) && e==EBADF);
    jet_fd_mkdir_at_raw(fd,&name,0700,&e); assert(e==EBADF);
    jet_fd_unlink_at_raw(fd,&name,false,&e); assert(e==EBADF);
    assert(jet_fd_readlink_at_raw(fd,&name,&byte,1,&e)==-1 && e==EBADF);
    jet_fd_link_at_raw(fd,fd,&name,&e); assert(e==EBADF);
    jet_fd_rename_at_raw(fd,&name,fd,&name,0,&e); assert(e==EBADF);
    jet_fd_close_raw(fd,&e,&z); assert(e==0 && !z); // idempotent
}
int main(void) {
    char root[]="/tmp/jet-files-XXXXXX"; assert(mkdtemp(root));
    char path[256], second[256], link[256], sub[256];
    snprintf(path,sizeof path,"%s/file",root);
    snprintf(second,sizeof second,"%s/second",root);
    snprintf(link,sizeof link,"%s/link",root);
    snprintf(sub,sizeof sub,"%s/sub",root);
    Text p=text(path), q=text(second), l=text(link), d=text(root), s=text(sub);
    W e=-1, facts[16]; bool z=true; char bytes[16384];
    FD *writer=jet_fd_open_raw(&p,O_WRONLY|O_CREAT|O_TRUNC,0600,&e);
    assert(writer && e==0 && writer->refs==1 && writer->cap==-8192);
    assert(fcntl((int)jet_fd_number_raw(writer),F_GETFD)&FD_CLOEXEC);
    Text *shown=jet_fd_path_raw(writer); assert(shown->len==p.len && !memcmp(shown->data,p.data,p.len)); jet_rt_string_drop(shown);
    assert(jet_fd_write_raw(writer,"a\r\nb\0c",6,&e,&z)==6 && e==0 && !z);
    jet_fd_stat_raw(writer,facts,&e); assert(e==0 && facts[7]==0); // buffered
    jet_fd_flush_raw(writer,&e,&z); assert(e==0 && !z);
    jet_fd_sync_raw(writer,false,&e); assert(e==0);
    jet_fd_sync_raw(writer,true,&e); assert(e==0);
    jet_fd_chmod_raw(writer,0640,&e); assert(e==0);
    jet_fd_stat_raw(writer,facts,&e); assert(e==0 && facts[7]==6 && (facts[2]&0777)==0640);
    struct stat expected; assert(!stat(path,&expected));
    assert((uint64_t)facts[0]==expected.st_dev && (uint64_t)facts[1]==expected.st_ino && facts[3]==(W)expected.st_nlink);
    assert(facts[4]==expected.st_uid && facts[5]==expected.st_gid && facts[12]==expected.st_ctim.tv_sec && facts[13]==expected.st_ctim.tv_nsec);
    assert(facts[14]==expected.st_blocks && facts[15]==expected.st_blksize);
    jet_fd_created_raw(writer,facts,&e); assert(e==0 || e==ENOSYS || e==EOPNOTSUPP); assert(facts[0]==0 || facts[0]==1);
    jet_fd_lock_raw(writer,LOCK_EX|LOCK_NB,&e); assert(e==0);
    jet_fd_lock_raw(writer,LOCK_UN,&e); assert(e==0);
    jet_fd_truncate_raw(writer,5,&e); assert(e==0);
    FD *alias=jet_fd_retain_raw(writer); assert(alias==writer && writer->refs==2);
    jet_fd_close_raw(writer,&e,&z); assert(e==0 && !z); closed(alias);
    jet_fd_drop_raw(writer); jet_fd_drop_raw(alias);
    FD *reader=jet_fd_open_raw(&p,O_RDONLY,0,&e); assert(reader && e==0);
    assert(jet_fd_read_raw(reader,bytes,2,&e)==2 && e==0 && !memcmp(bytes,"a\r",2));
    assert(jet_fd_seek_raw(reader,0,SEEK_CUR,&e,&z)==2 && e==0 && !z);
    assert(jet_fd_read_raw(reader,bytes,3,&e)==3 && !memcmp(bytes,"\nb\0",3));
    assert(jet_fd_read_raw(reader,bytes,1,&e)==0 && e==0);
    assert(jet_fd_seek_raw(reader,0,SEEK_SET,&e,&z)==0 && e==0);
    FD *duplicate=jet_fd_dup_raw(reader,&e); assert(duplicate && duplicate!=reader && e==0);
    jet_fd_drop_raw(duplicate);
    FD *directory=jet_fd_open_raw(&d,O_RDONLY|O_DIRECTORY,0,&e); assert(directory && e==0);
    Text member=text("file"), hard=text("hard"), dir=text("dir"), renamed=text("renamed");
    FD *at=jet_fd_open_at_raw(directory,&member,O_RDONLY|O_NOFOLLOW,0,&e); assert(at && e==0); jet_fd_drop_raw(at);
    assert(jet_fd_directory_raw(directory,bytes,sizeof bytes,&e)>0 && e==0);
    jet_fd_link_at_raw(reader,directory,&hard,&e);
    // FileHandles.rs's AT_EMPTY_PATH has the same privilege error on both tiers.
    assert(e==0 || e==EPERM || e==ENOENT);
    if (!e) { jet_fd_unlink_at_raw(directory,&hard,false,&e); assert(e==0); }
    jet_fd_mkdir_at_raw(directory,&dir,0700,&e); assert(e==0);
    jet_fd_rename_at_raw(directory,&dir,directory,&renamed,1,&e); assert(e==0);
    jet_fd_unlink_at_raw(directory,&renamed,true,&e); assert(e==0);
    jet_fs_link_raw(&p,&l,true,&e); assert(e==0);
    assert(jet_fs_readlink_raw(&l,bytes,sizeof bytes,&e)==p.len && e==0 && !memcmp(bytes,p.data,p.len));
    Text link_member=text("link"); assert(jet_fd_readlink_at_raw(directory,&link_member,bytes,sizeof bytes,&e)==p.len && e==0);
    jet_fs_stat_raw(&l,false,facts,&e); assert(e==0 && (facts[2]&S_IFMT)==S_IFLNK);
    jet_fs_created_raw(&p,true,facts,&e); assert(e==0 || e==ENOSYS || e==EOPNOTSUPP);
    jet_fs_unlink_raw(&l,false,&e); assert(e==0);
    jet_fs_link_raw(&p,&q,false,&e); assert(e==0);
    jet_fs_rename_raw(&q,&l,&e); assert(e==0);
    jet_fs_unlink_raw(&l,false,&e); assert(e==0);
    jet_fs_mkdir_raw(&s,0700,&e); assert(e==0);
    jet_fs_unlink_raw(&s,true,&e); assert(e==0);
    jet_fd_drop_raw(reader); jet_fd_drop_raw(directory);
    int pipes[2]; assert(!pipe(pipes));
    FD *r=jet_fd_adopt_raw(pipes[0],NULL,false), *w=jet_fd_adopt_unbuffered_raw(pipes[1],NULL);
    assert(jet_fd_write_unbuffered_raw(w,"bin\0",4,&e)==4 && e==0);
    assert(jet_fd_read_unbuffered_raw(r,bytes,4,&e)==4 && e==0 && !memcmp(bytes,"bin\0",4));
    jet_fd_drop_raw(r); jet_fd_drop_raw(w);
    jet_fs_unlink_raw(&p,false,&e); assert(e==0);
    assert(!jet_fd_open_raw(&p,O_RDONLY,0,&e) && e==ENOENT);
    jet_fs_stat_raw(&p,true,facts,&e); assert(e==ENOENT);
    jet_fs_rename_raw(&p,&q,&e); assert(e==ENOENT);
    jet_fs_link_raw(&p,&q,false,&e); assert(e==ENOENT);
    assert(jet_fs_readlink_raw(&p,bytes,1,&e)==-1 && e==ENOENT);
    jet_fs_unlink_raw(&p,false,&e); assert(e==ENOENT);
    jet_fs_unlink_raw(&d,true,&e); assert(e==0);
    puts("files raw: ok");
}
