// Real Jet providers only. No Rust layout reads.
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { unsigned char *data; int64_t len, capacity; } Text;
typedef struct { int64_t len, capacity; Text **data; } List;
typedef struct { int64_t cols, rows, mode; } Policy;
typedef struct { int64_t len, capacity; int64_t *data; } Words;
typedef struct {
    List *cmd; Text *cwd; int64_t env_clear; List *env_set, *env_remove;
    int64_t *in; int64_t out, err; int64_t *timeout, *output, *cpu, *memory, *files;
    int64_t detached; Policy *terminal; Text *wire; List *directories; int64_t parent_death; Words *cpu_affinity;
} Spec;
_Static_assert(sizeof(Spec)==152 && offsetof(Spec,terminal)==112 && offsetof(Spec,wire)==120 && offsetof(Spec,directories)==128 && offsetof(Spec,parent_death)==136 && offsetof(Spec,cpu_affinity)==144, "native Spec152");
_Static_assert(sizeof(List)==24 && sizeof(Policy)==24, "canonical nested carriers");
extern void jet_rt_alloc_init(void);
extern int64_t jet_rt_alloc_check_observe(int64_t, int64_t);
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
extern Spec *jet_process_spec_arg(Spec *,const Text *);
extern Spec *jet_process_spec_args_extend(Spec *,Text *const *,int64_t);
extern Spec *jet_process_spec_cwd(Spec *,const Text *);
extern Spec *jet_process_spec_env(Spec *,const Text *,const Text *);
extern Spec *jet_process_spec_env_remove(Spec *,const Text *);
extern Spec *jet_process_spec_env_clear(Spec *);
extern Spec *jet_process_spec_detached(Spec *);
extern Spec *jet_process_spec_stdin(Spec *,int64_t);
extern Spec *jet_process_spec_stdout(Spec *,int64_t);
extern Spec *jet_process_spec_stderr(Spec *,int64_t);
extern Spec *jet_process_spec_output_limit(Spec *,int64_t);
extern Spec *jet_process_spec_memory_limit(Spec *,int64_t);
extern Spec *jet_process_spec_open_file_limit(Spec *,int64_t);
extern Spec *jet_process_spec_timeout(Spec *,int64_t);
extern Spec *jet_process_spec_cpu_time_limit(Spec *,int64_t);
extern Spec *jet_process_spec_cpu_affinity(Spec *,const int64_t *,int64_t);
extern Spec *jet_process_spec_terminal(Spec *);
extern Spec *jet_process_spec_terminal_with_policy(Spec *,const Policy *);
static struct { void *p; int64_t bytes,align; unsigned frees; } watched[256];
static size_t count;
static unsigned intercepted;
// The checked heap reports every free it accepts here, the runtime's own
// included (jet_rt_alloc_check_observe), before it reclaims the block.
static void observe_free(void *p,int64_t bytes,int64_t align) {
    ++intercepted;
    for(size_t i=0;i<count;++i) if(watched[i].p==p) {
        assert(bytes==watched[i].bytes && align==watched[i].align);
        assert(++watched[i].frees==1);
    }
}
// A block watched again before it is freed is another owner of the same
// refcounted text: it stays one entry that must be freed exactly once.
static void watch(void *p,int64_t bytes,int64_t align) {
    assert(count<256);
    for(size_t i=0;i<count;++i) if(watched[i].p==p) { assert(!watched[i].frees && bytes==watched[i].bytes && align==watched[i].align); return; }
    watched[count].p=p; watched[count].bytes=bytes; watched[count].align=align; watched[count++].frees=0;
}
static void watch_text(Text *t) { if(t) { watch((char *)(t)-8,40,8); if(t->capacity) watch(t->data,t->capacity,1); } }
// Heap text is [count][data,len,cap,cache]; the owner count is the word before.
static int64_t owners(Text *t) { return *(int64_t *)((char *)t-8)&INT64_C(0x3fffffffffffffff); }
// An owned copy of t whose first byte differs: clones share t's bytes, so a
// differing field is a different text, never a write through a shared one.
static Text *flipped(Text *t) {
    char raw[64]; assert(t->len>0 && t->len<=64);
    memcpy(raw,t->data,(size_t)t->len); raw[0]^=1;
    return jet_rt_string_from_static(raw,t->len);
}
static void watch_list(List *list,bool pairs) {
    watch(list,24,8);
    if(list->capacity) watch(list->data,list->capacity*(pairs?16:8),8);
    for(int64_t i=0;i<list->len*(pairs?2:1);++i) watch_text(list->data[i]);
}
static void watch_spec(Spec *s) {
    watch(s,152,8); watch_list(s->cmd,false); watch_text(s->cwd);
    watch_list(s->env_set,true); watch_list(s->env_remove,false);
    if(s->in) watch(s->in,8,8);
    int64_t *limits[]={s->timeout,s->output,s->cpu,s->memory,s->files};
    for(size_t i=0;i<5;++i) if(limits[i]) watch(limits[i],8,8);
    if(s->terminal) watch(s->terminal,24,8);
    watch_text(s->wire);
    if(s->cpu_affinity) { watch(s->cpu_affinity,24,8); if(s->cpu_affinity->capacity) watch(s->cpu_affinity->data,s->cpu_affinity->capacity*8,8); }
}
static void done(void) { assert(intercepted>0); for(size_t i=0;i<count;++i) assert(watched[i].frees==1); count=0; }
static Text *text(const char *s) { return jet_rt_string_from_static(s,(int64_t)strlen(s)); }
static void bytes(Text *t,const char *s) { assert(t->len==(int64_t)strlen(s)); assert(memcmp(t->data,s,(size_t)t->len)==0); }
// Builders consume the spec they are given and return it changed in place.
static void replace(Spec **s,Spec *next) { assert(*s==next); }
// The compiler passes a spec another holder keeps as its own clone.
static Spec *own(const Spec *s) { return jet_rt_handle_clone_process_spec(s); }
static void defaults(Spec *s) {
    assert(s->cmd->len==0 && !s->cwd && !s->env_clear && s->env_set->len==0 && s->env_remove->len==0);
    assert(!s->in && s->out==2 && s->err==2 && !s->timeout && *s->output==67108864);
    assert(!s->cpu && !s->memory && !s->files && !s->detached && !s->terminal && !s->wire && !s->cpu_affinity);
    assert((const void *)jet_rt_handle_field_process_spec_env_clear(s)==(const void *)&s->env_clear);
    assert((const void *)jet_rt_handle_field_process_spec_detached(s)==(const void *)&s->detached);
    assert(!*jet_rt_handle_field_process_spec_env_clear(s) && !*jet_rt_handle_field_process_spec_detached(s));
}
static void clone_cases(void) {
    Spec *a=jet_std_process_cmd(NULL,0); defaults(a);
    Text *shown=jet_rt_handle_show_process_spec(a); bytes(shown,"ProcessSpec([])"); jet_rt_string_drop(shown);
    Text *arg=text("run"), *cwd=text("/secret"), *name=text("TOKEN"), *value=text("credential"), *other=text("PATH"), *gone=text("GONE");
    Spec *b=jet_process_spec_arg(own(a),arg); defaults(a); assert(b!=a && b->cmd->len==1 && b->cmd->data[0]==arg && owners(arg)==2); bytes(b->cmd->data[0],"run");
    replace(&b,jet_process_spec_cwd(b,cwd)); replace(&b,jet_process_spec_env(b,name,arg)); replace(&b,jet_process_spec_env(b,gone,value));
    // The last env for a name wins: its earlier row is forgotten and the new row is appended.
    replace(&b,jet_process_spec_env(b,name,value)); assert(b->env_set->len==2 && b->env_set->data[0]==gone && b->env_set->data[2]==name && b->env_set->data[3]==value);
    // env_remove forgets the earlier set row for its name.
    replace(&b,jet_process_spec_env_remove(b,gone)); assert(b->env_set->len==1 && b->env_set->data[0]==name && b->env_remove->len==1 && b->env_remove->data[0]==gone);
    replace(&b,jet_process_spec_env(b,other,arg)); assert(b->env_set->len==2 && b->env_set->data[2]==other && b->env_set->data[3]==arg);
    replace(&b,jet_process_spec_env_clear(b)); replace(&b,jet_process_spec_detached(b));
    replace(&b,jet_process_spec_stdin(b,0)); replace(&b,jet_process_spec_stdout(b,1)); replace(&b,jet_process_spec_stderr(b,0));
    int64_t timeout=jet_int_owned_from_i64(1999999), cpu=jet_int_owned_from_i64(-1999999);
    replace(&b,jet_process_spec_timeout(b,timeout)); assert(*b->timeout==1);
    replace(&b,jet_process_spec_cpu_time_limit(b,cpu)); assert(*b->cpu==0);
    jet_rt_int_drop(timeout); jet_rt_int_drop(cpu);
    replace(&b,jet_process_spec_output_limit(b,0)); replace(&b,jet_process_spec_memory_limit(b,42)); replace(&b,jet_process_spec_open_file_limit(b,17));
    replace(&b,jet_process_spec_terminal(b)); assert(b->terminal->cols==80 && b->terminal->rows==24 && b->terminal->mode==1);
    Policy policy={100,40,0}; replace(&b,jet_process_spec_terminal_with_policy(b,&policy)); assert(b->terminal!=&policy && b->terminal->cols==100);
    // Native owned wire fixture adds no constructor/export and is cloned/dropped normally.
    b->wire=text("hidden-policy");
    Spec *c=jet_rt_handle_clone_process_spec(b); assert(c!=b && jet_rt_handle_eq_process_spec(b,c));
    // The clone reallocates the record, lists and boxed words and shares every text by owner count.
    assert(c->cmd!=b->cmd && c->cmd->data!=b->cmd->data && c->cmd->data[0]==b->cmd->data[0] && owners(arg)==5);
    assert(c->cwd==b->cwd && owners(cwd)==3 && c->env_set->data[0]==b->env_set->data[0] && c->env_set->data[1]==b->env_set->data[1]);
    assert(c->env_remove->data[0]==b->env_remove->data[0] && owners(name)==3 && owners(value)==3 && owners(other)==3 && owners(gone)==3);
    assert(c->in!=b->in && c->output!=b->output && c->terminal!=b->terminal && c->wire==b->wire && owners(c->wire)==2);
    // Every stored field, including omitted secrets and optional presence, participates.
    Text **slots[]={&c->cmd->data[0],&c->cwd,&c->env_set->data[0],&c->env_set->data[1],&c->env_remove->data[0],&c->wire};
    for(size_t i=0;i<6;++i) { Text *kept=*slots[i],*diff=flipped(kept); *slots[i]=diff; assert(!jet_rt_handle_eq_process_spec(b,c)); *slots[i]=kept; jet_rt_string_drop(diff); }
    // Rows keep insertion order: the same rows swapped are a different spec.
    Text *swap=c->env_set->data[0]; c->env_set->data[0]=c->env_set->data[2]; c->env_set->data[2]=swap;
    swap=c->env_set->data[1]; c->env_set->data[1]=c->env_set->data[3]; c->env_set->data[3]=swap;
    assert(!jet_rt_handle_eq_process_spec(b,c));
    swap=c->env_set->data[0]; c->env_set->data[0]=c->env_set->data[2]; c->env_set->data[2]=swap;
    swap=c->env_set->data[1]; c->env_set->data[1]=c->env_set->data[3]; c->env_set->data[3]=swap;
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
    assert(owners(c->cwd)==2 && owners(c->wire)==1);
    bytes(c->cwd,"/secret"); bytes(c->env_set->data[1],"credential"); bytes(c->wire,"hidden-policy");
    jet_rt_handle_drop_process_spec(c);
    // Only the C owners remain; their releases free each text exactly once.
    assert(owners(arg)==1 && owners(cwd)==1 && owners(name)==1 && owners(value)==1 && owners(other)==1 && owners(gone)==1);
    jet_rt_string_drop(arg); jet_rt_string_drop(cwd); jet_rt_string_drop(name); jet_rt_string_drop(value);
    jet_rt_string_drop(other); jet_rt_string_drop(gone); done();
}
static void limits(void) {
    Spec *a=jet_std_process_cmd(NULL,0);
    int64_t hi=jet_int_owned_from_i64(INT64_MAX), lo=jet_int_owned_from_i64(INT64_MIN);
    int64_t above=jet_int_add(hi,1), below=jet_int_sub(lo,1);
    int64_t input[]={-1,0,1,hi,lo,above,below};
    int64_t expect[]={0,0,1,INT64_MAX,0,INT64_MAX,0};
    for(size_t i=0;i<7;++i) {
        Spec *b=jet_process_spec_output_limit(own(a),input[i]); assert(*b->output==expect[i]); jet_rt_handle_drop_process_spec(b);
        b=jet_process_spec_memory_limit(own(a),input[i]); assert(*b->memory==expect[i]); jet_rt_handle_drop_process_spec(b);
        b=jet_process_spec_open_file_limit(own(a),input[i]); assert(*b->files==expect[i]); jet_rt_handle_drop_process_spec(b);
        defaults(a);
    }
    int64_t nanos[]={INT64_MIN,-1000001,-1,0,999999,1000000,1999999,INT64_MAX};
    for(size_t i=0;i<8;++i) {
        int64_t d=jet_int_owned_from_i64(nanos[i]); int64_t ms=nanos[i]/1000000; if(ms<0) ms=0;
        Spec *b=jet_process_spec_timeout(own(a),d); assert(*b->timeout==ms); jet_rt_handle_drop_process_spec(b);
        b=jet_process_spec_cpu_time_limit(own(a),d); assert(*b->cpu==ms); jet_rt_handle_drop_process_spec(b); jet_rt_int_drop(d);
    }
    jet_rt_int_drop(hi); jet_rt_int_drop(lo); jet_rt_int_drop(above); jet_rt_int_drop(below); jet_rt_handle_drop_process_spec(a);
}
// `.cpu_affinity(cpus)` keeps the list as given, larger Ints saturated, for
// the launch check; the last call replaces it, and it takes part in clone,
// equality and drop.
static void affinity(void) {
    Spec *a=jet_std_process_cmd(NULL,0);
    int64_t hi=jet_int_owned_from_i64(INT64_MAX), lo=jet_int_owned_from_i64(INT64_MIN);
    int64_t above=jet_int_add(hi,1), below=jet_int_sub(lo,1);
    int64_t cpus[]={3,0,-2,above,below};
    Spec *b=jet_process_spec_cpu_affinity(own(a),cpus,5); defaults(a);
    Words *w=b->cpu_affinity; assert(w && w->len==5 && w->data[0]==3 && w->data[1]==0 && w->data[2]==-2 && w->data[3]==INT64_MAX && w->data[4]==-INT64_MAX);
    assert(!jet_rt_handle_eq_process_spec(a,b));
    Spec *c=jet_rt_handle_clone_process_spec(b); assert(c->cpu_affinity!=b->cpu_affinity && c->cpu_affinity->data!=w->data && jet_rt_handle_eq_process_spec(b,c));
    ++c->cpu_affinity->data[4]; assert(!jet_rt_handle_eq_process_spec(b,c)); --c->cpu_affinity->data[4];
    replace(&c,jet_process_spec_cpu_affinity(c,cpus,1)); assert(c->cpu_affinity->len==1 && c->cpu_affinity->data[0]==3 && !jet_rt_handle_eq_process_spec(b,c));
    Spec *empty=jet_process_spec_cpu_affinity(own(a),NULL,9); assert(empty->cpu_affinity && empty->cpu_affinity->len==0 && !jet_rt_handle_eq_process_spec(a,empty));
    watch_spec(a); watch_spec(b); watch_spec(c); watch_spec(empty);
    jet_rt_handle_drop_process_spec(a); jet_rt_handle_drop_process_spec(b); jet_rt_handle_drop_process_spec(c); jet_rt_handle_drop_process_spec(empty);
    jet_rt_int_drop(hi); jet_rt_int_drop(lo); jet_rt_int_drop(above); jet_rt_int_drop(below); done();
}
static void modes_and_debug(void) {
    static const unsigned char raw[]={'\0','\t','\n','\r','"','\\','\'',0xcc,0x81};
    Text *input=jet_rt_string_from_static(raw,(int64_t)sizeof(raw)); Text *plain=text("x"); Text *args[]={input,plain};
    Spec *a=jet_std_process_cmd(args,2); assert(a->cmd->data[0]==input && owners(input)==2);
    Text *shown=jet_rt_handle_show_process_spec(a);
    bytes(shown,"ProcessSpec([\"\\0\\t\\n\\r\\\"\\\\'\\u{301}\", \"x\"])"); jet_rt_string_drop(shown);
    Spec *extended=jet_process_spec_args_extend(own(a),args,2); assert(a->cmd->len==2 && extended->cmd->len==4);
    assert(extended->cmd->data[0]==input && extended->cmd->data[2]==input && owners(input)==4); jet_rt_handle_drop_process_spec(extended);
    extended=jet_process_spec_args_extend(own(a),NULL,9); assert(jet_rt_handle_eq_process_spec(a,extended)); jet_rt_handle_drop_process_spec(extended);
    extended=jet_process_spec_args_extend(own(a),args,-1); assert(jet_rt_handle_eq_process_spec(a,extended)); jet_rt_handle_drop_process_spec(extended);
    Spec *empty=jet_std_process_cmd(NULL,9); defaults(empty); jet_rt_handle_drop_process_spec(empty);
    empty=jet_std_process_cmd(args,-1); defaults(empty); jet_rt_handle_drop_process_spec(empty);
    for(int64_t mode=0;mode<3;++mode) {
        Spec *b=jet_process_spec_stdin(own(a),mode); assert(b->in && *b->in==mode && !a->in); jet_rt_handle_drop_process_spec(b);
        b=jet_process_spec_stdout(own(a),mode); assert(b->out==mode && a->out==2); jet_rt_handle_drop_process_spec(b);
        b=jet_process_spec_stderr(own(a),mode); assert(b->err==mode && a->err==2); jet_rt_handle_drop_process_spec(b);
    }
    assert(input->len==(int64_t)sizeof(raw) && !memcmp(input->data,raw,sizeof(raw)));
    jet_rt_handle_drop_process_spec(a); jet_rt_string_drop(input); jet_rt_string_drop(plain);
}
int main(int argc,char **argv) {
    assert(jet_rt_alloc_check_observe(0, (int64_t)(uintptr_t)observe_free) == 1);
    jet_rt_alloc_init();
    if(argc==2) {
        Spec *s=jet_std_process_cmd(NULL,0);
        if(!strcmp(argv[1],"stdin-range")) (void)jet_process_spec_stdin(s,-1);
        else if(!strcmp(argv[1],"stdout-range")) (void)jet_process_spec_stdout(s,3);
        else if(!strcmp(argv[1],"stderr-range")) (void)jet_process_spec_stderr(s,INT64_MAX);
        else return 64;
        assert(!"range stop returned");
    }
    clone_cases(); limits(); affinity(); modes_and_debug(); jet_rt_handle_drop_process_spec(NULL);
    puts("ProcessSpec producers/ownership/equality/render OK");
}
