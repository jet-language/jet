// Complete Jet runtime object + real C entry: no allocator-only/mocked leaves.
#define _GNU_SOURCE
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <signal.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/resource.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <termios.h>
#include <unistd.h>
typedef int64_t W;
typedef struct { unsigned char *data; W len, cap; } Text;
typedef struct { uint32_t domain,stage; W code; uint32_t kind,pad; Text *message; } Error;
typedef struct { W len,cap; void *data; } List;
typedef struct { W lock,pid,reaped,status,in,out,err,pty,refs; } Child;
typedef struct { W lock,fd,closed; Text *path; void *buffer; W cap,filled,pos,refs; } FD;
typedef struct { FD *fd; W target,cwd; } Dir;
typedef struct { W cols,rows,mode; } Terminal;
typedef struct { W count; char **column; W script_fallback,searched; } Candidates;
_Static_assert(sizeof(Error)==32 && sizeof(Child)==72 && sizeof(Dir)==24 && sizeof(Candidates)==32,"host ABI");
extern void jet_rt_host_program_start(void);
extern void *jet_rt_alloc(W,W);
extern void jet_rt_free(void *,W,W);
extern Text *jet_rt_string_clone(Text *);
extern void jet_rt_string_drop(Text *);
extern void *jet_process_spec_take_raw(W *);
extern void *jet_process_spec_clone_raw(void *);
extern void jet_process_spec_drop_raw(void *);
extern Child *jet_process_spawn_raw(void *,Candidates *,char **,char **,void *,Error *);
extern W fixture_process_exec_errno_class_raw(W);
extern W fixture_process_cpu_seconds_raw(W);
extern Child *jet_process_retain_raw(Child *);
extern void jet_process_drop_raw(Child *);
extern W jet_process_id_raw(Child *);
extern W jet_process_wait_raw(Child *,bool,W *,Error *);
extern void jet_process_kill_raw(Child *,W,bool,Error *);
extern W jet_process_read_raw(Child *,W,void *,W,Error *);
extern W jet_process_write_raw(Child *,W,const void *,W,Error *);
extern void jet_process_close_stream_raw(Child *,W,Error *);
extern FD *jet_process_take_stream_raw(Child *,W,Error *);
extern W jet_fd_read_raw(FD *,void *,W,Error *);
extern W jet_fd_write_unbuffered_raw(FD *,const void *,W,Error *);
extern void jet_fd_resize_raw(FD *,W,W,Error *);
extern void jet_process_resize_raw(Child *,W,W,Error *);
extern void jet_process_path_stat_raw(char *,W *,Error *);
extern void *jet_process_path_canonicalize_raw(char *,W *,W *,Error *);
extern Text *jet_host_os_error_text_raw(W,W);
extern FD *jet_fd_adopt_unbuffered_raw(W,Text *);
extern void jet_fd_drop_raw(FD *);
static Text text(char *s) { return (Text){(unsigned char *)s,(W)strlen(s),0}; }
static void good(Error *e) { assert(e->domain==0 && e->stage==0 && e->code==0 && !e->kind && !e->pad && !e->message); }
static void posix(Error *e,int stage,int code) { assert(e->domain==1 && e->stage==(unsigned)stage && e->code==code && !e->kind && !e->message); }
static int helper(int argc,char **argv) {
    if (!strcmp(argv[1],"echo")) { char b[8]; assert(read(0,b,4)==4); assert(write(1,b,4)==4); const char *v=getenv("BINARY"); assert(v && (unsigned char)*v==255); assert(write(1,v,1)==1); assert(write(2,"err",3)==3); return 7; }
    if (!strcmp(argv[1],"wait")) { assert(getpgrp()==getpid()); assert(write(1,"ready",5)==5); char b; assert(read(0,&b,1)==1); return 9; }
    if (!strcmp(argv[1],"mapped")) { assert(argc==5); struct stat a,b; assert(!fstat(198,&a) && !fstat(199,&b)); assert(a.st_ino==(ino_t)strtoull(argv[2],0,10) && b.st_ino==(ino_t)strtoull(argv[3],0,10)); char *cwd=getcwd(NULL,0); assert(cwd && !strcmp(cwd,argv[4])); free(cwd); assert(write(1,"mapped",6)==6); return 0; }
    if (!strcmp(argv[1],"limits")) { assert(getsid(0)==getpid()); struct rlimit r; assert(!getrlimit(RLIMIT_CPU,&r) && r.rlim_cur==2 && r.rlim_max==3); assert(!getrlimit(RLIMIT_AS,&r) && r.rlim_cur==268435456); assert(!getrlimit(RLIMIT_NOFILE,&r) && r.rlim_cur==64); assert(write(1,"limits",6)==6); return 0; }
    if (!strcmp(argv[1],"pty")) { struct winsize size; struct termios mode; assert(isatty(0)&&isatty(1)&&isatty(2)); assert(!ioctl(0,TIOCGWINSZ,&size) && size.ws_col==61 && size.ws_row==23); assert(!tcgetattr(0,&mode) && !(mode.c_lflag&(ECHO|ICANON))); assert(write(1,"ready",5)==5); char b; assert(read(0,&b,1)==1); assert(!ioctl(0,TIOCGWINSZ,&size) && size.ws_col==72 && size.ws_row==31); assert(write(1,"resized",7)==7); return 0; }
    abort();
}
static void read_exact(Child *child,W stream,char *out,size_t n) { Error e; size_t at=0; while(at<n) { W count=jet_process_read_raw(child,stream,out+at,(W)(n-at),&e); good(&e); assert(count>0); at+=(size_t)count; } }
static Child *launch(W *spec,char *exe,char **argv,char **env,Error *e) { char *column[]={exe}; Candidates candidates={1,column,0,0}; return jet_process_spawn_raw(spec,&candidates,argv,env,NULL,e); }
static Child *spawn(W *spec,char *exe,char **argv,char **env) { Error e; Child *c=launch(spec,exe,argv,env,&e); good(&e); assert(c && c->refs==1 && jet_process_id_raw(c)>0); return c; }
static void write_fixture(char *path,const char *bytes,int mode) { int fd=open(path,O_CREAT|O_TRUNC|O_WRONLY,mode);assert(fd>=0&&write(fd,bytes,strlen(bytes))==(ssize_t)strlen(bytes));assert(!close(fd)); }
static int copy_fixture(char *source,char *target) { int in=open(source,O_RDONLY),out=open(target,O_CREAT|O_TRUNC|O_WRONLY,0700);assert(in>=0&&out>=0);char chunk[16384];ssize_t count;while((count=read(in,chunk,sizeof chunk))>0){ssize_t at=0;while(at<count){ssize_t written=write(out,chunk+at,(size_t)(count-at));assert(written>0);at+=written;}}assert(count==0&&!close(in)&&!close(out));return 0; }
static void wait_code(Child *c,int code) { Error e; W status; assert(jet_process_wait_raw(c,false,&status,&e)==jet_process_id_raw(c)); good(&e); assert(WIFEXITED((int)status)&&WEXITSTATUS((int)status)==code); assert(jet_process_wait_raw(c,true,&status,&e)==jet_process_id_raw(c)); good(&e); jet_process_kill_raw(c,SIGKILL,false,&e); good(&e); }
static void *wait_thread(void *pointer) { Child *c=pointer; Error e; W status; assert(jet_process_wait_raw(c,false,&status,&e)==jet_process_id_raw(c)); good(&e); assert(WEXITSTATUS((int)status)==9); jet_process_drop_raw(c); return NULL; }
int main(int argc,char **argv) {
    if(argc>1) return helper(argc,argv);
    jet_rt_host_program_start();
    const int retry[]={ENOENT,ESTALE,ENOTDIR,ENODEV,ETIMEDOUT};
    for(size_t i=0;i<sizeof retry/sizeof retry[0];i++) assert(fixture_process_exec_errno_class_raw(-retry[i])==1);
    assert(fixture_process_exec_errno_class_raw(-EACCES)==2);
    const int stop[]={ENOEXEC,ELOOP,ETXTBSY,E2BIG,ENOMEM,ENAMETOOLONG,EINVAL};
    for(size_t i=0;i<sizeof stop/sizeof stop[0];i++) assert(fixture_process_exec_errno_class_raw(-stop[i])==0);
    assert(fixture_process_cpu_seconds_raw(0)==0 && fixture_process_cpu_seconds_raw(1)==1);
    assert(fixture_process_cpu_seconds_raw(1000)==1 && fixture_process_cpu_seconds_raw(1001)==2);
    assert(fixture_process_cpu_seconds_raw(INT64_MAX)==INT64_MAX/1000);
    assert(fixture_process_cpu_seconds_raw(INT64_MAX-998)==INT64_MAX/1000);
    char executable[4096]; ssize_t n=readlink("/proc/self/exe",executable,sizeof(executable)-1); assert(n>0); executable[n]=0;
    W spec[27]={0}; spec[7]=1; spec[8]=2; spec[9]=2; spec[10]=2;
    char *env[]={"BINARY=\xff",NULL}, *echo[]={executable,"echo",NULL}; Error e; char bytes[32];
    Child *c=spawn(spec, executable, echo, env);
    assert(jet_process_write_raw(c,0,"b\0in",4,&e)==4); good(&e); jet_process_close_stream_raw(c,0,&e); good(&e);
    read_exact(c,1,bytes,5); assert(!memcmp(bytes,"b\0in\xff",5)); read_exact(c,2,bytes,3); assert(!memcmp(bytes,"err",3));
    assert(jet_process_read_raw(c,1,bytes,1,&e)==0); good(&e); wait_code(c,7); jet_process_resize_raw(c,80,24,&e); posix(&e,0,EBADF); jet_process_drop_raw(c);
    char *waiting[]={executable,"wait",NULL}; c=spawn(spec, executable, waiting, env); read_exact(c,1,bytes,5);
    W status=-1; assert(jet_process_wait_raw(c,true,&status,&e)==0); good(&e);
    pthread_t first,second; assert(!pthread_create(&first,NULL,wait_thread,jet_process_retain_raw(c))); assert(!pthread_create(&second,NULL,wait_thread,jet_process_retain_raw(c)));
    // WNOHANG is available even while another thread owns a blocking wait.
    assert(jet_process_wait_raw(c,true,&status,&e)==0); good(&e); assert(jet_process_write_raw(c,0,"x",1,&e)==1); good(&e);
    assert(!pthread_join(first,NULL)&&!pthread_join(second,NULL)); wait_code(c,9); jet_process_drop_raw(c);
    c=spawn(spec, executable, waiting, env);read_exact(c,1,bytes,5);jet_process_kill_raw(c,SIGTERM,false,&e);good(&e);assert(jet_process_wait_raw(c,false,&status,&e)>0);good(&e);assert(WIFSIGNALED((int)status)&&WTERMSIG((int)status)==SIGTERM);jet_process_drop_raw(c);
    spec[21]=1;c=spawn(spec, executable, waiting, env);read_exact(c,1,bytes,5);jet_process_kill_raw(c,SIGTERM,true,&e);good(&e);assert(jet_process_wait_raw(c,false,&status,&e)>0);good(&e);assert(WIFSIGNALED((int)status));jet_process_drop_raw(c);spec[21]=0;
    c=spawn(spec, executable, echo, env);FD *input=jet_process_take_stream_raw(c,0,&e);good(&e);FD *output=jet_process_take_stream_raw(c,1,&e);good(&e);FD *error_stream=jet_process_take_stream_raw(c,2,&e);good(&e);assert(input&&output&&error_stream);assert(!jet_process_take_stream_raw(c,1,&e));good(&e);pid_t orphan=(pid_t)jet_process_id_raw(c);jet_process_drop_raw(c);
    assert(jet_fd_write_unbuffered_raw(input,"b\0in",4,&e)==4);good(&e);jet_fd_drop_raw(input);
    size_t out_at=0;while(out_at<5){W got=jet_fd_read_raw(output,bytes+out_at,(W)(5-out_at),&e);good(&e);assert(got>0);out_at+=(size_t)got;}assert(!memcmp(bytes,"b\0in\xff",5));jet_fd_drop_raw(output);jet_fd_drop_raw(error_stream);int orphan_status;assert(waitpid(orphan,&orphan_status,0)==orphan&&WEXITSTATUS(orphan_status)==7);
    char root[]="/tmp/jet-spawn-XXXXXX"; assert(mkdtemp(root)); char other[4096],bad[4096],symlink1[4096],symlink2[4096],nonutf[4096];
    snprintf(other,sizeof other,"%s/other",root); assert(!mkdir(other,0700));
    int a=open(root,O_RDONLY|O_DIRECTORY),b=open(other,O_RDONLY|O_DIRECTORY); assert(a>=0&&b>=0); assert(dup2(a,198)==198&&dup2(b,199)==199); close(a);close(b);
    FD *fa=jet_fd_adopt_unbuffered_raw(198,NULL), *fb=jet_fd_adopt_unbuffered_raw(199,NULL); Dir maps[2]={{fa,199,0},{fb,198,1}}; List dirs={2,2,maps}; spec[26]=(W)(intptr_t)&dirs;
    struct stat astat,bstat; assert(!fstat(198,&astat)&&!fstat(199,&bstat)); char ai[32],bi[32]; snprintf(ai,sizeof ai,"%llu",(unsigned long long)bstat.st_ino); snprintf(bi,sizeof bi,"%llu",(unsigned long long)astat.st_ino);
    char *mapped[]={executable,"mapped",ai,bi,other,NULL}; c=spawn(spec, executable, mapped, env); read_exact(c,1,bytes,6); assert(!memcmp(bytes,"mapped",6)); wait_code(c,0); jet_process_drop_raw(c); spec[26]=0; jet_fd_drop_raw(fa);jet_fd_drop_raw(fb);
    spec[21]=1;
    spec[15]=1;spec[16]=1001;spec[17]=1;spec[18]=268435456;spec[19]=1;spec[20]=64; char *limits[]={executable,"limits",NULL}; c=spawn(spec, executable, limits, env);read_exact(c,1,bytes,6);assert(!memcmp(bytes,"limits",6));wait_code(c,0);jet_process_drop_raw(c);spec[15]=spec[17]=spec[19]=0;
    spec[21]=0;
    spec[21]=1;
    Terminal terminal={61,23,0}; spec[22]=1;spec[23]=(W)(intptr_t)&terminal;char *pty[]={executable,"pty",NULL};c=spawn(spec, executable, pty, env);read_exact(c,1,bytes,5);jet_process_resize_raw(c,72,31,&e);good(&e);assert(jet_process_write_raw(c,0,"x",1,&e)==1);good(&e);read_exact(c,3,bytes,7);assert(!memcmp(bytes,"resized",7));assert(jet_process_read_raw(c,1,bytes,1,&e)==0);good(&e);wait_code(c,0);jet_process_close_stream_raw(c,3,&e);good(&e);jet_process_drop_raw(c);spec[22]=0;spec[23]=0;
    spec[21]=0;
    spec[21]=1;
    spec[22]=1;spec[23]=(W)(intptr_t)&terminal;c=spawn(spec, executable, pty, env);input=jet_process_take_stream_raw(c,0,&e);good(&e);output=jet_process_take_stream_raw(c,1,&e);good(&e);assert(!jet_process_take_stream_raw(c,2,&e));good(&e);FD *control=jet_process_take_stream_raw(c,3,&e);good(&e);assert(input&&output&&control);jet_fd_resize_raw(control,72,31,&e);good(&e);assert(jet_fd_read_raw(output,bytes,5,&e)==5);good(&e);assert(jet_fd_write_unbuffered_raw(input,"x",1,&e)==1);good(&e);assert(jet_fd_read_raw(output,bytes,7,&e)==7);good(&e);wait_code(c,0);jet_process_drop_raw(c);jet_fd_resize_raw(control,80,24,&e);good(&e);jet_fd_drop_raw(input);jet_fd_drop_raw(output);jet_fd_drop_raw(control);spec[22]=0;spec[23]=0;
    spec[21]=0;
    snprintf(bad,sizeof bad,"%s/missing",root);Text missing=text(bad);assert(!launch(spec, bad, echo, env, &e));posix(&e,7,ENOENT);
    int file=open(bad,O_CREAT|O_WRONLY,0600);assert(file>=0);close(file);assert(!launch(spec, bad, echo, env, &e));posix(&e,7,EACCES);assert(!unlink(bad));
    spec[1]=1;spec[2]=(W)(intptr_t)&missing;assert(!launch(spec, executable, echo, env, &e));posix(&e,3,ENOENT);spec[1]=0;spec[2]=0;
    // Real exec errors drive the ordered candidate loop; no mocked syscall.
    char inaccessible[4096],absent[4096],looping[4096],script[4096],interpreter[4096],not_directory[8192];
    snprintf(inaccessible,sizeof inaccessible,"%s/denied",root);write_fixture(inaccessible,"denied",0600);
    snprintf(absent,sizeof absent,"%s/absent",root);snprintf(not_directory,sizeof not_directory,"%s/child",inaccessible);
    char *column[]={inaccessible,executable};Candidates candidates={2,column,0,1};
    // Owner #33 deliberately replaces glibc's stale-errno empty-PATH pathology.
    Candidates empty={0,NULL,0,1};assert(!jet_process_spawn_raw(spec,&empty,waiting,env,NULL,&e));posix(&e,7,ENOENT);
    spec[1]=1;spec[2]=(W)(intptr_t)&missing;assert(!jet_process_spawn_raw(spec,&empty,waiting,env,NULL,&e));posix(&e,3,ENOENT);spec[1]=0;spec[2]=0;
    c=jet_process_spawn_raw(spec,&candidates,waiting,env,NULL,&e);good(&e);assert(c);read_exact(c,1,bytes,5);assert(jet_process_write_raw(c,0,"x",1,&e)==1);good(&e);wait_code(c,9);jet_process_drop_raw(c);
    column[1]=absent;assert(!jet_process_spawn_raw(spec,&candidates,waiting,env,NULL,&e));posix(&e,7,EACCES);
    snprintf(looping,sizeof looping,"%s/loop",root);assert(!symlink("loop",looping));column[0]=looping;column[1]=executable;assert(!jet_process_spawn_raw(spec,&candidates,waiting,env,NULL,&e));posix(&e,7,ELOOP);
    snprintf(interpreter,sizeof interpreter,"%s/interpreter",root);write_fixture(interpreter,"#!/missing-jet-interpreter\n",0700);
    char *continue_column[]={not_directory,interpreter,executable};candidates.count=3;candidates.column=continue_column;
    c=jet_process_spawn_raw(spec,&candidates,waiting,env,NULL,&e);good(&e);assert(c);read_exact(c,1,bytes,5);assert(jet_process_write_raw(c,0,"x",1,&e)==1);good(&e);wait_code(c,9);jet_process_drop_raw(c);
    snprintf(script,sizeof script,"%s/script",root);write_fixture(script,"printf script; exit 6\n",0700);char *script_column[]={script};char *script_argv[]={script,"argument",NULL};candidates.count=1;candidates.column=script_column;
    assert(!jet_process_spawn_raw(spec,&candidates,script_argv,env,NULL,&e));posix(&e,7,ENOEXEC);
    candidates.script_fallback=1;c=jet_process_spawn_raw(spec,&candidates,script_argv,env,NULL,&e);good(&e);assert(c);read_exact(c,1,bytes,6);assert(!memcmp(bytes,"script",6));wait_code(c,6);jet_process_drop_raw(c);
    assert(!unlink(inaccessible)&&!unlink(looping)&&!unlink(interpreter)&&!unlink(script));
    // Exact realpath OS semantics: chain, symlink/.., raw non-UTF bytes, errors.
    snprintf(symlink1,sizeof symlink1,"%s/link1",root);snprintf(symlink2,sizeof symlink2,"%s/link2",root);assert(!symlink("other",symlink1)&&!symlink("link1",symlink2));
    W length,capacity; void *canonical=jet_process_path_canonicalize_raw(symlink2,&length,&capacity,&e);good(&e);assert(capacity==4096&&length==(W)strlen(other)&&!memcmp(canonical,other,(size_t)length));jet_rt_free(canonical,capacity,1);
    char through[8192];snprintf(through,sizeof through,"%s/..",symlink2);canonical=jet_process_path_canonicalize_raw(through,&length,&capacity,&e);good(&e);assert(length==(W)strlen(root)&&!memcmp(canonical,root,(size_t)length));jet_rt_free(canonical,capacity,1);
    snprintf(nonutf,sizeof nonutf,"%s/\xff",root);assert(!copy_fixture(executable,nonutf));canonical=jet_process_path_canonicalize_raw(nonutf,&length,&capacity,&e);good(&e);assert(length==(W)strlen(nonutf)&&!memcmp(canonical,nonutf,(size_t)length));char *raw_waiting[]={nonutf,"wait",NULL};c=spawn(spec,canonical,raw_waiting,env);read_exact(c,1,bytes,5);assert(jet_process_write_raw(c,0,"x",1,&e)==1);good(&e);wait_code(c,9);jet_process_drop_raw(c);jet_rt_free(canonical,capacity,1);
    W facts[16];jet_process_path_stat_raw(nonutf,facts,&e);good(&e);assert((facts[2]&S_IFMT)==S_IFREG);assert(!jet_process_path_canonicalize_raw(bad,&length,&capacity,&e));posix(&e,0,ENOENT);
    char too_long[5000];memset(too_long,'x',sizeof too_long-1);too_long[sizeof too_long-1]=0;assert(!jet_process_path_canonicalize_raw(too_long,&length,&capacity,&e));posix(&e,0,ENAMETOOLONG);
    Text *message=jet_host_os_error_text_raw(1,ENOENT);assert(message && message->len>0);jet_rt_string_drop(message);
    // Reviewed private carrier transfer/clone retains directories and deep E4s.
    W owned[27]={0};Text borrowed=text(root);owned[1]=1;owned[2]=(W)(intptr_t)jet_rt_string_clone(&borrowed);void *packed=jet_process_spec_take_raw(owned),*clone=jet_process_spec_clone_raw(packed);jet_process_spec_drop_raw(packed);jet_process_spec_drop_raw(clone);
    assert(!unlink(nonutf)&&!unlink(symlink1)&&!unlink(symlink2)&&!rmdir(other)&&!rmdir(root));
    puts("process raw: ok");
}
