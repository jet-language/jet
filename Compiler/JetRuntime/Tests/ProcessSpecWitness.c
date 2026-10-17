// Real Jet providers only; link -Wl,--wrap=jet_rt_free. No Rust layout reads.
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { unsigned char *data; int64_t len, capacity; } Text;
typedef struct { int64_t len, capacity; Text **data; } List;
typedef struct { int64_t cols, rows, mode; } Policy;
typedef struct { int64_t ns; } Duration;
typedef struct {
    List *cmd; Text *cwd; int64_t env_clear; List *env_set, *env_remove;
    int64_t *in; int64_t out, err; int64_t *timeout, *output, *cpu, *memory, *files;
    int64_t detached; Policy *terminal; Text *wire; List *directories;
} Spec;
_Static_assert(sizeof(Spec)==136 && offsetof(Spec,terminal)==112 && offsetof(Spec,wire)==120 && offsetof(Spec,directories)==128, "native Spec136");
_Static_assert(sizeof(List)==24 && sizeof(Policy)==24 && sizeof(Duration)==8, "canonical nested carriers");
extern void jet_rt_alloc_init(void);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern Text *jet_rt_string_from_static(const void *,int64_t);
extern void jet_rt_string_drop(Text *);
extern int64_t jet_int_owned_from_i64(int64_t);
extern int64_t jet_int_add(int64_t,int64_t);
extern int64_t jet_int_sub(int64_t,int64_t);
extern void jet_rt_int_drop(int64_t);
extern Spec *jet_std_process_cmd(Text *const *,int64_t);
extern Spec *jet_rt_handle_clone_process_spec(const Spec *);
extern void jet_rt_handle_drop_process_spec(Spec *);
extern bool jet_rt_handle_eq_process_spec(const Spec *,const Spec *);
extern Text *jet_rt_handle_show_process_spec(const Spec *);
extern const bool *jet_rt_handle_field_process_spec_env_clear(const Spec *);
extern const bool *jet_rt_handle_field_process_spec_detached(const Spec *);
extern Spec *jet_process_spec_arg(const Spec *,const Text *);
extern Spec *jet_process_spec_args_extend(const Spec *,Text *const *,int64_t);
extern Spec *jet_process_spec_cwd(const Spec *,const Text *);
extern Spec *jet_process_spec_env(const Spec *,const Text *,const Text *);
extern Spec *jet_process_spec_env_remove(const Spec *,const Text *);
extern Spec *jet_process_spec_env_clear(const Spec *);
extern Spec *jet_process_spec_detached(const Spec *);
extern Spec *jet_process_spec_stdin(const Spec *,int64_t);
extern Spec *jet_process_spec_stdout(const Spec *,int64_t);
extern Spec *jet_process_spec_stderr(const Spec *,int64_t);
extern Spec *jet_process_spec_output_limit(const Spec *,int64_t);
extern Spec *jet_process_spec_memory_limit(const Spec *,int64_t);
extern Spec *jet_process_spec_open_file_limit(const Spec *,int64_t);
extern Spec *jet_process_spec_timeout(const Spec *,const Duration *);
extern Spec *jet_process_spec_cpu_time_limit(const Spec *,const Duration *);
extern Spec *jet_process_spec_terminal(const Spec *);
extern Spec *jet_process_spec_terminal_with_policy(const Spec *,const Policy *);
static struct { void *p; int64_t bytes,align; unsigned frees; } watched[256];
static size_t count;
static unsigned intercepted;
void __wrap_jet_rt_free(void *p,int64_t bytes,int64_t align) {
    ++intercepted;
    for(size_t i=0;i<count;++i) if(watched[i].p==p) {
        assert(bytes==watched[i].bytes && align==watched[i].align);
        assert(++watched[i].frees==1);
    }
    __real_jet_rt_free(p,bytes,align);
}
static void watch(void *p,int64_t bytes,int64_t align) {
    assert(count<256);
    for(size_t i=0;i<count;++i) assert(watched[i].p!=p);
    watched[count].p=p; watched[count].bytes=bytes; watched[count].align=align; watched[count++].frees=0;
}
static void watch_text(Text *t) { if(t) { watch((char *)(t)-8,32,8); if(t->capacity) watch(t->data,t->capacity,1); } }
static void watch_list(List *list,bool pairs) {
    watch(list,24,8);
    if(list->capacity) watch(list->data,list->capacity*(pairs?16:8),8);
    for(int64_t i=0;i<list->len*(pairs?2:1);++i) watch_text(list->data[i]);
}
static void watch_spec(Spec *s) {
    watch(s,136,8); watch_list(s->cmd,false); watch_text(s->cwd);
    watch_list(s->env_set,true); watch_list(s->env_remove,false);
    if(s->in) watch(s->in,8,8);
    int64_t *limits[]={s->timeout,s->output,s->cpu,s->memory,s->files};
    for(size_t i=0;i<5;++i) if(limits[i]) watch(limits[i],8,8);
    if(s->terminal) watch(s->terminal,24,8);
    watch_text(s->wire);
}
static void done(void) { assert(intercepted>0); for(size_t i=0;i<count;++i) assert(watched[i].frees==1); count=0; }
static Text *text(const char *s) { return jet_rt_string_from_static(s,(int64_t)strlen(s)); }
static void bytes(Text *t,const char *s) { assert(t->len==(int64_t)strlen(s)); assert(memcmp(t->data,s,(size_t)t->len)==0); }
static void replace(Spec **s,Spec *next) { assert(*s!=next); jet_rt_handle_drop_process_spec(*s); *s=next; }
static void defaults(Spec *s) {
    assert(s->cmd->len==0 && !s->cwd && !s->env_clear && s->env_set->len==0 && s->env_remove->len==0);
    assert(!s->in && s->out==2 && s->err==2 && !s->timeout && *s->output==67108864);
    assert(!s->cpu && !s->memory && !s->files && !s->detached && !s->terminal && !s->wire);
    assert((const void *)jet_rt_handle_field_process_spec_env_clear(s)==(const void *)&s->env_clear);
    assert((const void *)jet_rt_handle_field_process_spec_detached(s)==(const void *)&s->detached);
    assert(!*jet_rt_handle_field_process_spec_env_clear(s) && !*jet_rt_handle_field_process_spec_detached(s));
}
static void clone_cases(void) {
    Spec *a=jet_std_process_cmd(NULL,0); defaults(a);
    Text *shown=jet_rt_handle_show_process_spec(a); bytes(shown,"ProcessSpec([])"); jet_rt_string_drop(shown);
    Text *arg=text("run"), *cwd=text("/secret"), *name=text("TOKEN"), *value=text("credential");
    Spec *b=jet_process_spec_arg(a,arg); defaults(a); assert(b->cmd->len==1 && b->cmd->data[0]!=arg); bytes(b->cmd->data[0],"run");
    replace(&b,jet_process_spec_cwd(b,cwd)); replace(&b,jet_process_spec_env(b,name,value));
    replace(&b,jet_process_spec_env(b,name,arg)); assert(b->env_set->len==2); // duplicates preserve insertion order
    replace(&b,jet_process_spec_env_remove(b,name)); replace(&b,jet_process_spec_env_clear(b)); replace(&b,jet_process_spec_detached(b));
    replace(&b,jet_process_spec_stdin(b,0)); replace(&b,jet_process_spec_stdout(b,1)); replace(&b,jet_process_spec_stderr(b,0));
    Duration timeout={1999999}, cpu={-1999999};
    replace(&b,jet_process_spec_timeout(b,&timeout)); assert(*b->timeout==1);
    replace(&b,jet_process_spec_cpu_time_limit(b,&cpu)); assert(*b->cpu==0);
    replace(&b,jet_process_spec_output_limit(b,0)); replace(&b,jet_process_spec_memory_limit(b,42)); replace(&b,jet_process_spec_open_file_limit(b,17));
    replace(&b,jet_process_spec_terminal(b)); assert(b->terminal->cols==80 && b->terminal->rows==24 && b->terminal->mode==1);
    Policy policy={100,40,0}; replace(&b,jet_process_spec_terminal_with_policy(b,&policy)); assert(b->terminal!=&policy && b->terminal->cols==100);
    // Native owned wire fixture adds no constructor/export and is cloned/dropped normally.
    b->wire=text("hidden-policy");
    Spec *c=jet_rt_handle_clone_process_spec(b); assert(c!=b && jet_rt_handle_eq_process_spec(b,c));
    assert(c->cmd!=b->cmd && c->cmd->data!=b->cmd->data && c->cmd->data[0]!=b->cmd->data[0]);
    assert(c->cwd!=b->cwd && c->env_set->data[0]!=b->env_set->data[0] && c->env_set->data[1]!=b->env_set->data[1]);
    assert(c->env_remove->data[0]!=b->env_remove->data[0] && c->in!=b->in && c->output!=b->output && c->terminal!=b->terminal && c->wire!=b->wire);
    // Every stored field, including omitted secrets and optional presence, participates.
    Text *texts[]={c->cmd->data[0],c->cwd,c->env_set->data[0],c->env_set->data[1],c->env_remove->data[0],c->wire};
    for(size_t i=0;i<6;++i) { texts[i]->data[0]^=1; assert(!jet_rt_handle_eq_process_spec(b,c)); texts[i]->data[0]^=1; }
    Text *swap=c->env_set->data[0]; c->env_set->data[0]=c->env_set->data[2]; c->env_set->data[2]=swap;
    // Same duplicate names: change values' order instead.
    swap=c->env_set->data[1]; c->env_set->data[1]=c->env_set->data[3]; c->env_set->data[3]=swap;
    assert(!jet_rt_handle_eq_process_spec(b,c)); swap=c->env_set->data[1]; c->env_set->data[1]=c->env_set->data[3]; c->env_set->data[3]=swap;
    c->env_clear|=INT64_C(0x123400); c->detached|=INT64_C(0x567800);
    assert(jet_rt_handle_eq_process_spec(b,c)); // Byte-lending Bool slots ignore padding.
    Spec *normalized=jet_rt_handle_clone_process_spec(c);
    assert(normalized->env_clear==1 && normalized->detached==1 && jet_rt_handle_eq_process_spec(b,normalized));
    jet_rt_handle_drop_process_spec(normalized); c->env_clear=1; c->detached=1;
    int64_t *words[]={&c->env_clear,c->in,&c->out,&c->err,c->timeout,c->output,c->cpu,c->memory,c->files,&c->detached,&c->terminal->cols,&c->terminal->rows,&c->terminal->mode};
    for(size_t i=0;i<13;++i) { ++*words[i]; assert(!jet_rt_handle_eq_process_spec(b,c)); --*words[i]; }
    int64_t *saved=c->in; c->in=NULL; assert(!jet_rt_handle_eq_process_spec(b,c)); c->in=saved;
    saved=c->output; c->output=NULL; assert(!jet_rt_handle_eq_process_spec(b,c)); c->output=saved;
    assert(jet_rt_handle_eq_process_spec(b,c));
    shown=jet_rt_handle_show_process_spec(c); bytes(shown,"ProcessSpec([\"run\"])"); jet_rt_string_drop(shown);
    watch_spec(a); watch_spec(b); watch_spec(c);
    jet_rt_handle_drop_process_spec(a); jet_rt_handle_drop_process_spec(b);
    bytes(c->cwd,"/secret"); bytes(c->env_set->data[1],"credential"); bytes(c->wire,"hidden-policy");
    jet_rt_handle_drop_process_spec(c); done();
    jet_rt_string_drop(arg); jet_rt_string_drop(cwd); jet_rt_string_drop(name); jet_rt_string_drop(value);
}
static void limits(void) {
    Spec *a=jet_std_process_cmd(NULL,0);
    int64_t hi=jet_int_owned_from_i64(INT64_MAX), lo=jet_int_owned_from_i64(INT64_MIN);
    int64_t above=jet_int_add(hi,1), below=jet_int_sub(lo,1);
    int64_t input[]={-1,0,1,hi,lo,above,below};
    int64_t expect[]={0,0,1,INT64_MAX,0,INT64_MAX,0};
    for(size_t i=0;i<7;++i) {
        Spec *b=jet_process_spec_output_limit(a,input[i]); assert(*b->output==expect[i]); jet_rt_handle_drop_process_spec(b);
        b=jet_process_spec_memory_limit(a,input[i]); assert(*b->memory==expect[i]); jet_rt_handle_drop_process_spec(b);
        b=jet_process_spec_open_file_limit(a,input[i]); assert(*b->files==expect[i]); jet_rt_handle_drop_process_spec(b);
        defaults(a);
    }
    int64_t nanos[]={INT64_MIN,-1000001,-1,0,999999,1000000,1999999,INT64_MAX};
    for(size_t i=0;i<8;++i) {
        Duration d={nanos[i]}; int64_t ms=nanos[i]/1000000; if(ms<0) ms=0;
        Spec *b=jet_process_spec_timeout(a,&d); assert(*b->timeout==ms); jet_rt_handle_drop_process_spec(b);
        b=jet_process_spec_cpu_time_limit(a,&d); assert(*b->cpu==ms); jet_rt_handle_drop_process_spec(b); assert(d.ns==nanos[i]);
    }
    jet_rt_int_drop(hi); jet_rt_int_drop(lo); jet_rt_int_drop(above); jet_rt_int_drop(below); jet_rt_handle_drop_process_spec(a);
}
static void modes_and_debug(void) {
    static const unsigned char raw[]={'\0','\t','\n','\r','"','\\','\'',0xcc,0x81};
    Text *input=jet_rt_string_from_static(raw,(int64_t)sizeof(raw)); Text *plain=text("x"); Text *args[]={input,plain};
    Spec *a=jet_std_process_cmd(args,2); assert(a->cmd->data[0]!=input);
    Text *shown=jet_rt_handle_show_process_spec(a);
    bytes(shown,"ProcessSpec([\"\\0\\t\\n\\r\\\"\\\\'\\u{301}\", \"x\"])"); jet_rt_string_drop(shown);
    Spec *extended=jet_process_spec_args_extend(a,args,2); assert(a->cmd->len==2 && extended->cmd->len==4);
    assert(extended->cmd->data[0]!=extended->cmd->data[2]); jet_rt_handle_drop_process_spec(extended);
    extended=jet_process_spec_args_extend(a,NULL,9); assert(jet_rt_handle_eq_process_spec(a,extended)); jet_rt_handle_drop_process_spec(extended);
    extended=jet_process_spec_args_extend(a,args,-1); assert(jet_rt_handle_eq_process_spec(a,extended)); jet_rt_handle_drop_process_spec(extended);
    Spec *empty=jet_std_process_cmd(NULL,9); defaults(empty); jet_rt_handle_drop_process_spec(empty);
    empty=jet_std_process_cmd(args,-1); defaults(empty); jet_rt_handle_drop_process_spec(empty);
    for(int64_t mode=0;mode<3;++mode) {
        Spec *b=jet_process_spec_stdin(a,mode); assert(b->in && *b->in==mode && !a->in); jet_rt_handle_drop_process_spec(b);
        b=jet_process_spec_stdout(a,mode); assert(b->out==mode && a->out==2); jet_rt_handle_drop_process_spec(b);
        b=jet_process_spec_stderr(a,mode); assert(b->err==mode && a->err==2); jet_rt_handle_drop_process_spec(b);
    }
    assert(input->len==(int64_t)sizeof(raw) && !memcmp(input->data,raw,sizeof(raw)));
    jet_rt_handle_drop_process_spec(a); jet_rt_string_drop(input); jet_rt_string_drop(plain);
}
int main(int argc,char **argv) {
    jet_rt_alloc_init();
    if(argc==2) {
        Spec *s=jet_std_process_cmd(NULL,0);
        if(!strcmp(argv[1],"stdin-range")) (void)jet_process_spec_stdin(s,-1);
        else if(!strcmp(argv[1],"stdout-range")) (void)jet_process_spec_stdout(s,3);
        else if(!strcmp(argv[1],"stderr-range")) (void)jet_process_spec_stderr(s,INT64_MAX);
        else return 64;
        assert(!"range stop returned");
    }
    clone_cases(); limits(); modes_and_debug(); jet_rt_handle_drop_process_spec(NULL);
    puts("ProcessSpec producers/ownership/equality/render OK");
}
