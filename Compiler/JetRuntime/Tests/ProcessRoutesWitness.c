// Canonical raw adapter and logical-environment ABI witness; real kernel fds.
#define _GNU_SOURCE
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
typedef struct { unsigned char *data; int64_t len,capacity; } Text;
typedef struct { int64_t len,capacity; void *data; } List;
typedef struct { void *owner; int64_t child,as_cwd; } Directory;
typedef struct { List *cmd; Text *cwd; int64_t clear; List *set,*remove; int64_t *in,out,err,*timeout,*output,*cpu,*memory,*files,detached; void *terminal; Text *wire; List *directories; } Spec;
_Static_assert(sizeof(Spec)==136 && sizeof(Directory)==24,"native carriers");
extern void jet_rt_alloc_init(void);
extern void jet_rt_host_arguments(int64_t,char **,char **);
extern Text *jet_rt_string_from_static(const void *,int64_t);
extern void jet_rt_string_drop(Text *);
extern int64_t jet_rt_logical_env_get(Text *);
extern Spec *jet_process_spec_fields_raw(Text **,int64_t,bool,Text *,bool,Text **,int64_t,Text **,int64_t,Text **,int64_t,bool,int64_t,int64_t,int64_t,bool,int64_t,bool,int64_t,bool,int64_t,bool,int64_t,bool,int64_t,bool,bool,void *,bool,Text *);
extern Spec *jet_rt_handle_clone_process_spec(Spec *);
extern void jet_rt_handle_drop_process_spec(Spec *);
extern bool jet_rt_handle_eq_process_spec(Spec *,Spec *);
extern void jet_process_spec_with_directory_raw(Spec *,int64_t,int64_t,bool,Spec **,bool *,int64_t *,int64_t *);
extern int64_t jet_std_env_try_set(Text *,Text *,void **);
extern int64_t jet_std_env_unset(Text *,bool *,void **);
extern void jet_rt_handle_drop_env_error(void *);
static Text *text(const char *s){ return jet_rt_string_from_static(s,(int64_t)strlen(s)); }
static void equal(Text *t,const char *s){assert(t&&t->len==(int64_t)strlen(s)&&!memcmp(t->data,s,strlen(s)));}
static Spec *fields(Text **cmd,Text **names,Text **values,Text *wire){return jet_process_spec_fields_raw(cmd,1,false,NULL,true,names,2,values,1,NULL,0,true,1,2,1,true,23,true,99,false,0,false,0,false,0,false,false,NULL,true,wire);}
static void adapters(void){
 Text *arg=text("echo"),*name=text("A"),*ignored=text("ignored"),*value=text("v"),*wire=text("policy");
 Text *cmd[]={arg},*names[]={name,ignored},*values[]={value}; Spec *s=fields(cmd,names,values,wire);
 assert(s->clear==1&&s->cmd->len==1&&s->set->len==1&&s->remove->len==0&&!s->directories);
 assert(((Text **)s->cmd->data)[0]!=arg);equal(((Text **)s->cmd->data)[0],"echo");
 assert(s->in&&*s->in==1&&s->out==2&&s->err==1&&*s->timeout==23&&*s->output==99&&!s->cpu&&!s->memory&&!s->files&&!s->terminal);
 assert(s->wire!=wire);equal(s->wire,"policy");
 int fd=open("/tmp",O_RDONLY|O_DIRECTORY|O_CLOEXEC);assert(fd>=0);
 struct { unsigned char before; bool value; unsigned char after; } supported={0x5a,false,0xa5};
 Spec *attached=NULL;int64_t assigned=-2,error=-3;
 jet_process_spec_with_directory_raw(s,fd,100,true,&attached,&supported.value,&assigned,&error);
 assert(supported.before==0x5a&&supported.after==0xa5&&supported.value&&error==0&&assigned>=100);
 assert(!s->directories&&attached->directories->len==1);
 Directory *row=attached->directories->data;assert(row->owner&&row->child==100&&row->as_cwd==1);
 assert(fcntl((int)assigned,F_GETFD)&FD_CLOEXEC);
 Spec *clone=jet_rt_handle_clone_process_spec(attached);assert(jet_rt_handle_eq_process_spec(attached,clone));
 jet_rt_handle_drop_process_spec(attached);assert(fcntl((int)assigned,F_GETFD)>=0);
 jet_rt_handle_drop_process_spec(clone);errno=0;assert(fcntl((int)assigned,F_GETFD)==-1&&errno==EBADF);
 Spec *failed=NULL;jet_process_spec_with_directory_raw(s,-1,100,false,&failed,&supported.value,&assigned,&error);
 assert(supported.value&&assigned==-1&&error==EBADF&&!failed->directories&&jet_rt_handle_eq_process_spec(s,failed));
 jet_rt_handle_drop_process_spec(failed);jet_rt_handle_drop_process_spec(s);close(fd);
 jet_rt_string_drop(arg);jet_rt_string_drop(name);jet_rt_string_drop(ignored);jet_rt_string_drop(value);jet_rt_string_drop(wire);
}
static void environment(void){
 Text *name=text("JET_PROCESS_ROUTES_WITNESS_PRIVATE"),*value=text("logical"),*bad=text("bad=name");void *error=(void *)(uintptr_t)0x1234;
 assert(!getenv("JET_PROCESS_ROUTES_WITNESS_PRIVATE"));assert(jet_std_env_try_set(name,value,&error)==1&&error==(void *)(uintptr_t)0x1234);
 Text *got=(Text *)(uintptr_t)jet_rt_logical_env_get(name);equal(got,"logical");jet_rt_string_drop(got);assert(!getenv("JET_PROCESS_ROUTES_WITNESS_PRIVATE"));
 struct { unsigned char before;bool removed;unsigned char after; } slot={0x5a,false,0xa5};
 assert(jet_std_env_unset(name,&slot.removed,&error)==1&&slot.removed&&slot.before==0x5a&&slot.after==0xa5);
 assert(jet_std_env_unset(name,&slot.removed,&error)==1&&!slot.removed&&slot.before==0x5a&&slot.after==0xa5);
 assert(jet_std_env_try_set(bad,value,&error)==0&&*(int64_t *)error==0);jet_rt_handle_drop_env_error(error);
 const char invalid_bytes[]={'v',0,'x'};Text *invalid=jet_rt_string_from_static(invalid_bytes,3);
 assert(jet_std_env_try_set(name,invalid,&error)==0&&*(int64_t *)error==1);jet_rt_handle_drop_env_error(error);
 assert(jet_std_env_unset(bad,&slot.removed,&error)==0&&*(int64_t *)error==0);jet_rt_handle_drop_env_error(error);
 jet_rt_string_drop(invalid);jet_rt_string_drop(name);jet_rt_string_drop(value);jet_rt_string_drop(bad);
}
extern char **environ;
int main(int argc,char **argv){jet_rt_alloc_init();jet_rt_host_arguments(argc,argv,environ);adapters();environment();puts("process routes: ok");return 0;}
