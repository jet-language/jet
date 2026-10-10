// Real native producer witness; link --wrap=jet_rt_free, never a fake Inner.
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>
typedef struct { unsigned char *data; int64_t length,capacity; } Text;
typedef struct { int64_t index; double coefficient; } Term;
typedef struct { Term terms[16]; int64_t count; double constant; int64_t present,id; } Expr;
typedef struct { void *inner; int64_t index; } Handle;
typedef struct { int64_t length,capacity; Text **data; } TextList;
_Static_assert(sizeof(Expr)==288 && sizeof(Handle)==16 && sizeof(TextList)==24,"native Layout ABI");
extern void jet_rt_alloc_init(void),jet_rt_thread_init(void),jet_rt_thread_attach(void),jet_rt_thread_detach(void),jet_rt_layout_init(void),jet_rt_tls_reset_attempt(void);
extern void __real_jet_rt_free(void *,int64_t,int64_t),jet_rt_free(void *,int64_t,int64_t);
extern Text *jet_rt_string_new(const unsigned char *,int64_t);
extern void jet_rt_string_drop(Text *);
extern Handle *jet_rt_layout_new(Text *),*jet_rt_layout_compare(Expr *,Expr *,int64_t),*jet_rt_layout_strength(Handle *,int64_t);
extern Expr *jet_rt_layout_variable(Handle *,Text *,Text *),*jet_rt_layout_constant(double),*jet_rt_layout_add(Expr *,Expr *),*jet_rt_layout_sub(Expr *,Expr *);
extern void jet_rt_layout_suggest(Handle *,Expr *,double);
extern double jet_rt_layout_value(Handle *,Expr *);
extern bool jet_rt_layout_is_feasible(Handle *);
extern TextList *jet_rt_layout_conflict(Handle *);
extern Handle *jet_rt_handle_clone_handle(Handle *),*jet_rt_handle_clone_constraint(Handle *);
extern Expr *jet_rt_handle_clone_lin_expr(Expr *);
extern void jet_rt_handle_drop_handle(Handle *),jet_rt_handle_drop_constraint(Handle *),jet_rt_handle_drop_lin_expr(Expr *);
static void *watched[128];
static unsigned watches,frees;
void __wrap_jet_rt_free(void *p,int64_t bytes,int64_t align) {
    for(unsigned i=0;i<watches;++i) if(p==watched[i]) {
        assert(bytes==152 && align==8); watched[i]=NULL; ++frees;
    }
    __real_jet_rt_free(p,bytes,align);
}
static void watch(Handle *h) { assert(watches<128); watched[watches++]=h->inner; }
static Text *text(const char *s) { return jet_rt_string_new((const unsigned char *)s,(int64_t)strlen(s)); }
static Expr *variable(Handle *h,int index) {
    char name[32]; snprintf(name,sizeof name,"v%d",index);
    Text *b=text(name),*a=text("width"); Expr *e=jet_rt_layout_variable(h,b,a);
    jet_rt_string_drop(b); jet_rt_string_drop(a); return e;
}
static void conflict(const char *name,Handle *h) {
    TextList *list=jet_rt_layout_conflict(h);
    printf("c %s %lld",name,(long long)list->length);
    for(int64_t i=0;i<list->length;++i) {
        Text *t=list->data[i]; printf("|%.*s",(int)t->length,t->data); jet_rt_string_drop(t);
    }
    putchar('\n'); if(list->capacity) jet_rt_free(list->data,list->capacity*8,8);
    jet_rt_free(list,24,8);
}
static void ownership(void) {
    Text *name=text("ownership"); Handle *h=jet_rt_layout_new(name); jet_rt_string_drop(name); watch(h);
    Expr *x=variable(h,0),*again=variable(h,0),*ten=jet_rt_layout_constant(10);
    assert(x->count==1 && again->terms[0].index==x->terms[0].index && x->present==1);
    for(unsigned i=1;i<16;++i) assert(x->terms[i].index==0 && x->terms[i].coefficient==0);
    Expr *xc=jet_rt_handle_clone_lin_expr(x); assert(xc!=x && !memcmp(xc,x,288));
    Handle *hc=jet_rt_handle_clone_handle(h); assert(hc!=h && hc->inner==h->inner && hc->index==h->index);
    Handle *c=jet_rt_layout_compare(x,ten,2),*cc=jet_rt_handle_clone_constraint(c);
    assert(c!=cc && c->inner==cc->inner && c->index==cc->index);
    jet_rt_handle_drop_handle(h); jet_rt_handle_drop_lin_expr(x); jet_rt_handle_drop_lin_expr(again);
    jet_rt_handle_drop_constraint(c); jet_rt_handle_drop_lin_expr(ten);
    assert(fabs(jet_rt_layout_value(hc,xc)-10)<1e-7);
    assert(jet_rt_layout_strength(cc,3)==cc);
    jet_rt_layout_suggest(hc,xc,27); assert(fabs(jet_rt_layout_value(hc,xc)-27)<1e-7);
    jet_rt_tls_reset_attempt(); assert(fabs(jet_rt_layout_value(hc,xc)-27)<1e-7);
    // Registry releases its ref at thread exit, independently retained owners survive.
    unsigned before=frees; jet_rt_thread_detach(); assert(frees==before);
    jet_rt_thread_attach(); assert(fabs(jet_rt_layout_value(hc,xc)-27)<1e-7);
    jet_rt_handle_drop_handle(hc); assert(frees==before);
    jet_rt_handle_drop_constraint(cc); assert(frees==before+1);
    jet_rt_handle_drop_lin_expr(xc);
}
static void fatal(const char *mode) {
    Text *name=text("fatal"); Handle *h=jet_rt_layout_new(name); jet_rt_string_drop(name);
    Expr *x=variable(h,0);
    if(!strcmp(mode,"overflow-add") || !strcmp(mode,"overflow-compare")) {
        Expr *sum=jet_rt_handle_clone_lin_expr(x);
        for(int i=1;i<17;++i) {
            Expr *v=variable(h,i);
            if(i==16 && !strcmp(mode,"overflow-compare")) { (void)jet_rt_layout_compare(sum,v,2); exit(9); }
            Expr *next=jet_rt_layout_add(sum,v); jet_rt_handle_drop_lin_expr(sum); jet_rt_handle_drop_lin_expr(v); sum=next;
        }
        exit(9);
    }
    assert(!strcmp(mode,"infeasible-value"));
    Expr *ten=jet_rt_layout_constant(10),*five=jet_rt_layout_constant(5);
    (void)jet_rt_layout_compare(x,ten,1); (void)jet_rt_layout_compare(x,five,0);
    (void)jet_rt_layout_value(h,x); exit(9);
}
static void multi_edit(void) {
    // Seed HashMap traversal is unspecified. Reversing the edit insertion
    // order must still agree for these unique optima (not an arbitrary tie).
    double solved[2][4];
    for(int reversed=0;reversed<2;++reversed) {
        Text *name=text("two-edits"); Handle *h=jet_rt_layout_new(name);
        jet_rt_string_drop(name); watch(h);
        Expr *x=variable(h,0),*y=variable(h,1);
        jet_rt_layout_suggest(h,reversed?y:x,reversed?7:3);
        jet_rt_layout_suggest(h,reversed?x:y,reversed?3:7);
        assert(fabs(jet_rt_layout_value(h,x)-3)<1e-7);
        assert(fabs(jet_rt_layout_value(h,y)-7)<1e-7);
        Expr *sum=jet_rt_layout_add(x,y),*ten=jet_rt_layout_constant(10);
        Handle *constraint=jet_rt_layout_compare(sum,ten,2);
        solved[reversed][0]=jet_rt_layout_value(h,x);
        solved[reversed][1]=jet_rt_layout_value(h,y);
        jet_rt_layout_suggest(h,reversed?y:x,reversed?6:4);
        jet_rt_layout_suggest(h,reversed?x:y,reversed?4:6);
        solved[reversed][2]=jet_rt_layout_value(h,x);
        solved[reversed][3]=jet_rt_layout_value(h,y);
        assert(fabs(solved[reversed][2]-4)<1e-7);
        assert(fabs(solved[reversed][3]-6)<1e-7);
        assert(jet_rt_layout_is_feasible(h));
        jet_rt_handle_drop_constraint(constraint); jet_rt_handle_drop_handle(h);
        jet_rt_handle_drop_lin_expr(x); jet_rt_handle_drop_lin_expr(y);
        jet_rt_handle_drop_lin_expr(sum); jet_rt_handle_drop_lin_expr(ten);
    }
    for(unsigned i=0;i<4;++i) assert(fabs(solved[0][i]-solved[1][i])<1e-7);
}
int main(int argc,char **argv) {
    jet_rt_alloc_init(); jet_rt_thread_init(); jet_rt_layout_init();
    if(argc==3 && !strcmp(argv[1],"--fatal")) { fatal(argv[2]); return 9; }
    assert(argc==2); ownership(); multi_edit();
    FILE *in=fopen(argv[1],"r"); assert(in);
    Expr *e[64]={0}; Handle *c[64]={0},*h=NULL; char name[64]={0},line[256];
    while(fgets(line,sizeof line,in)) {
        char op[32]={0}; int a=0,b=0,d=0,r=0; double value=0;
        if(sscanf(line,"%31s",op)!=1) continue;
        if(!strcmp(op,"case")) {
            assert(!h); assert(sscanf(line,"%*s %63s",name)==1);
            Text *n=text(name); h=jet_rt_layout_new(n); jet_rt_string_drop(n); watch(h);
        } else if(!strcmp(op,"var")) { assert(sscanf(line,"%*s %d",&a)==1); e[a]=variable(h,a);
        } else if(!strcmp(op,"const")) { assert(sscanf(line,"%*s %d %lf",&a,&value)==2); e[a]=jet_rt_layout_constant(value);
        } else if(!strcmp(op,"add") || !strcmp(op,"sub")) {
            assert(sscanf(line,"%*s %d %d %d",&a,&b,&d)==3); e[a]=!strcmp(op,"add")?jet_rt_layout_add(e[b],e[d]):jet_rt_layout_sub(e[b],e[d]);
        } else if(!strcmp(op,"cmp")) { assert(sscanf(line,"%*s %d %d %d %d",&a,&b,&d,&r)==4); c[a]=jet_rt_layout_compare(e[b],e[d],r);
        } else if(!strcmp(op,"strength")) { assert(sscanf(line,"%*s %d %d",&a,&r)==2); c[a]=jet_rt_layout_strength(c[a],r);
        } else if(!strcmp(op,"suggest")) { assert(sscanf(line,"%*s %d %lf",&a,&value)==2); jet_rt_layout_suggest(h,e[a],value);
        } else if(!strcmp(op,"value")) { assert(sscanf(line,"%*s %d",&a)==1); printf("v %s %d %.9f\n",name,a,jet_rt_layout_value(h,e[a]));
        } else if(!strcmp(op,"feasible")) { printf("f %s %d\n",name,jet_rt_layout_is_feasible(h));
        } else if(!strcmp(op,"conflict")) { conflict(name,h);
        } else if(!strcmp(op,"constraint_feasible")) { assert(sscanf(line,"%*s %d",&a)==1); printf("f %s %d\n",name,jet_rt_layout_is_feasible(c[a]));
        } else if(!strcmp(op,"constraint_conflict")) { assert(sscanf(line,"%*s %d",&a)==1); conflict(name,c[a]);
        } else { assert(!strcmp(op,"end"));
            for(unsigned i=0;i<64;++i) { if(e[i]) jet_rt_handle_drop_lin_expr(e[i]); if(c[i]) jet_rt_handle_drop_constraint(c[i]); e[i]=NULL; c[i]=NULL; }
            unsigned before=frees; jet_rt_handle_drop_handle(h); h=NULL; assert(frees==before);
        }
    }
    assert(!h && !ferror(in)); fclose(in);
    assert(frees==1); jet_rt_thread_detach(); assert(frees==watches);
    puts("Layout6 ownership/registry/clone/drop OK"); return 0;
}
