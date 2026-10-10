// Link against real Jet objects with -Wl,--wrap=jet_rt_free; no provider mocks.
// Render oracle: rustc 1.97.1 (8bab26f4f 2026-07-14), f64 Display/Debug.
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { double real,imaginary; } Complex;
typedef struct { double min_width,min_height,max_width,max_height; } Size;
typedef struct { uint64_t state; } Rng;
typedef struct { unsigned char *data; int64_t length,capacity; } Text;
_Static_assert(sizeof(Complex)==16 && offsetof(Complex,imaginary)==8,"Complex16");
_Static_assert(sizeof(Size)==32 && offsetof(Size,max_height)==24,"Size32");
_Static_assert(sizeof(Rng)==8,"Rng8");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern void jet_rt_string_drop(Text *);
extern Text *jet_rt_float_debug(double);
extern Text *jet_rt_float_to_string(double);
extern Complex *jet_rt_record_new_jet_complex(double,double);
extern Complex *jet_rt_handle_clone_jet_complex(Complex *);
extern void jet_rt_handle_drop_jet_complex(Complex *);
extern bool jet_rt_handle_eq_jet_complex(Complex *,Complex *);
extern double jet_rt_record_get_jet_complex_real(Complex *);
extern double jet_rt_record_get_jet_complex_imaginary(Complex *);
extern Text *jet_rt_handle_show_jet_complex(Complex *);
extern Text *jet_rt_handle_display_jet_complex(Complex *);
extern Text *jet_rt_handle_debug_jet_complex(Complex *);
extern Size *jet_rt_record_new_jet_size_constraint(double,double,double,double);
extern Size *jet_rt_handle_clone_jet_size_constraint(Size *);
extern void jet_rt_handle_drop_jet_size_constraint(Size *);
extern bool jet_rt_handle_eq_jet_size_constraint(Size *,Size *);
extern double jet_rt_record_get_jet_size_constraint_min_width(Size *);
extern double jet_rt_record_get_jet_size_constraint_min_height(Size *);
extern double jet_rt_record_get_jet_size_constraint_max_width(Size *);
extern double jet_rt_record_get_jet_size_constraint_max_height(Size *);
extern Rng *jet_rt_handle_clone_rng(Rng *);
extern void jet_rt_handle_drop_rng(Rng *);
extern bool jet_rt_handle_eq_rng(Rng *,Rng *);
extern Text *jet_rt_handle_show_rng(Rng *);
static struct { void *p; int64_t bytes,align; unsigned frees; } watched[24];
static size_t count;
static unsigned interceptions;
void __wrap_jet_rt_free(void *p,int64_t bytes,int64_t align) {
    ++interceptions;
    for(size_t i=0;i<count;++i) if(watched[i].p==p) {
        assert(bytes==watched[i].bytes && align==watched[i].align);
        assert(++watched[i].frees==1);
    }
    __real_jet_rt_free(p,bytes,align);
}
static void watch(void *p,int64_t bytes,int64_t align) {
    assert(p && count<24);
    for(size_t i=0;i<count;++i) assert(watched[i].p!=p);
    watched[count].p=p; watched[count].bytes=bytes; watched[count].align=align;
    watched[count++].frees=0;
}
static void done(void) { assert(interceptions); for(size_t i=0;i<count;++i) assert(watched[i].frees==1); count=0; }
static void watch_text(Text *t) { watch(t,24,8); if(t->capacity) watch(t->data,t->capacity,1); }
static void text_is(Text *t,const char *expected) {
    assert(t && t->length==(int64_t)strlen(expected));
    assert(!t->length || memcmp(t->data,expected,(size_t)t->length)==0);
}
static double from_bits(uint64_t bits) { double d; memcpy(&d,&bits,8); return d; }
static uint64_t bits(double d) { uint64_t b; memcpy(&b,&d,8); return b; }
static void complex_render(double real,double imaginary,const char *expected) {
    Complex *a=jet_rt_record_new_jet_complex(real,imaginary),*b=jet_rt_handle_clone_jet_complex(a);
    assert(a!=b && memcmp(a,b,16)==0);
    assert(bits(jet_rt_record_get_jet_complex_real(a))==bits(real));
    assert(bits(jet_rt_record_get_jet_complex_imaginary(a))==bits(imaginary));
    Text *s=jet_rt_handle_show_jet_complex(a),*d=jet_rt_handle_display_jet_complex(a),*g=jet_rt_handle_debug_jet_complex(a);
    assert(s!=d && s!=g && d!=g);
    text_is(s,expected); text_is(d,expected); text_is(g,expected);
    watch(a,16,8); watch(b,16,8); jet_rt_handle_drop_jet_complex(a); jet_rt_handle_drop_jet_complex(b); done();
    text_is(s,expected); watch_text(s); watch_text(d); watch_text(g);
    jet_rt_string_drop(s); jet_rt_string_drop(d); jet_rt_string_drop(g); done();
}
static void equality(void) {
    Complex *a=jet_rt_record_new_jet_complex(1,2),*b=jet_rt_handle_clone_jet_complex(a);
    Size *s=jet_rt_record_new_jet_size_constraint(1,2,3,4),*t=jet_rt_handle_clone_jet_size_constraint(s);
    assert(s!=t && memcmp(s,t,32)==0);
    assert(jet_rt_record_get_jet_size_constraint_min_width(s)==1 && jet_rt_record_get_jet_size_constraint_min_height(s)==2);
    assert(jet_rt_record_get_jet_size_constraint_max_width(s)==3 && jet_rt_record_get_jet_size_constraint_max_height(s)==4);
    assert(jet_rt_handle_eq_jet_complex(a,b) && jet_rt_handle_eq_jet_size_constraint(s,t));
    double *af[]={&a->real,&a->imaginary},*bf[]={&b->real,&b->imaginary};
    double *sf[]={&s->min_width,&s->min_height,&s->max_width,&s->max_height};
    double *tf[]={&t->min_width,&t->min_height,&t->max_width,&t->max_height};
    for(size_t i=0;i<2;++i) {
        double saved=*af[i]; *bf[i]+=1; assert(!jet_rt_handle_eq_jet_complex(a,b));
        *af[i]=from_bits(UINT64_C(0xfff8000000000123)); *bf[i]=*af[i];
        assert(!jet_rt_handle_eq_jet_complex(a,a) && !jet_rt_handle_eq_jet_complex(a,b));
        *af[i]=-0.0; *bf[i]=0.0; assert(jet_rt_handle_eq_jet_complex(a,b)); *af[i]=saved; *bf[i]=saved;
    }
    for(size_t i=0;i<4;++i) {
        double saved=*sf[i]; *tf[i]+=1; assert(!jet_rt_handle_eq_jet_size_constraint(s,t));
        *sf[i]=from_bits(UINT64_C(0x7ff8000000000123)); *tf[i]=*sf[i];
        assert(!jet_rt_handle_eq_jet_size_constraint(s,s) && !jet_rt_handle_eq_jet_size_constraint(s,t));
        *sf[i]=-0.0; *tf[i]=0.0; assert(jet_rt_handle_eq_jet_size_constraint(s,t)); *sf[i]=saved; *tf[i]=saved;
    }
    s->max_height=from_bits(UINT64_C(0xfff8000000000456));
    Size *u=jet_rt_handle_clone_jet_size_constraint(s);
    assert(u!=s && memcmp(s,u,32)==0 && bits(jet_rt_record_get_jet_size_constraint_max_height(u))==UINT64_C(0xfff8000000000456));
    watch(a,16,8); watch(b,16,8); watch(s,32,8); watch(t,32,8); watch(u,32,8);
    jet_rt_handle_drop_jet_complex(a); jet_rt_handle_drop_jet_complex(b);
    jet_rt_handle_drop_jet_size_constraint(s); jet_rt_handle_drop_jet_size_constraint(t); jet_rt_handle_drop_jet_size_constraint(u); done();
    jet_rt_handle_drop_jet_complex(NULL); jet_rt_handle_drop_jet_size_constraint(NULL);
}
static void rngs(void) {
    const uint64_t states[]={0,1,UINT64_C(0x8000000000000000),UINT64_MAX};
    for(size_t i=0;i<4;++i) {
        Rng *a=jet_rt_alloc(8,8); a->state=states[i]; Rng *b=jet_rt_handle_clone_rng(a);
        assert(a!=b && b->state==states[i] && jet_rt_handle_eq_rng(a,b));
        b->state^=1; assert(!jet_rt_handle_eq_rng(a,b)); b->state^=1;
        Text *s=jet_rt_handle_show_rng(a); text_is(s,"RNG { .. }");
        watch(a,8,8); watch(b,8,8); watch_text(s);
        jet_rt_handle_drop_rng(a); jet_rt_handle_drop_rng(b); jet_rt_string_drop(s); done();
    }
    jet_rt_handle_drop_rng(NULL);
}
static void old_debug(double value,const char *expected) {
    Text *a=jet_rt_float_to_string(value),*b=jet_rt_float_debug(value);
    assert(a!=b); text_is(a,expected); text_is(b,expected);
    watch_text(a); watch_text(b); jet_rt_string_drop(a); jet_rt_string_drop(b); done();
}
int main(void) {
    jet_rt_alloc_init(); equality(); rngs();
    complex_render(1,2,"1 + 2i"); complex_render(-0.0,-0.0,"-0 - 0i");
    complex_render(1e16,1e21,"10000000000000000 + 1000000000000000000000i");
    // Fixed decimals copied from the pinned Rust oracle, not C snprintf.
    char extreme[700]; size_t at=0;
    extreme[at++]='0'; extreme[at++]='.'; memset(extreme+at,'0',323); at+=323;
    extreme[at++]='5'; memcpy(extreme+at," + ",3); at+=3;
    memcpy(extreme+at,"17976931348623157",17); at+=17; memset(extreme+at,'0',292); at+=292;
    extreme[at++]='i'; extreme[at]=0;
    complex_render(from_bits(1),from_bits(UINT64_C(0x7fefffffffffffff)),extreme);
    complex_render(from_bits(UINT64_C(0x7ff8000000000123)),from_bits(UINT64_C(0xfff8000000000456)),"NaN - NaNi");
    complex_render(from_bits(UINT64_C(0xfff8000000000123)),from_bits(UINT64_C(0x7ff8000000000456)),"NaN + NaNi");
    complex_render(from_bits(UINT64_C(0x7ff0000000000000)),from_bits(UINT64_C(0xfff0000000000000)),"inf - infi");
    old_debug(1,"1.0"); old_debug(-0.0,"-0.0"); old_debug(1e16,"1e16"); old_debug(1e21,"1e21");
    old_debug(from_bits(1),"5e-324"); old_debug(from_bits(UINT64_C(0x7fefffffffffffff)),"1.7976931348623157e308");
    old_debug(from_bits(UINT64_C(0x7ff8000000000123)),"NaN"); old_debug(from_bits(UINT64_C(0x7ff0000000000000)),"inf");
    puts("MathGlue ownership/equality/render OK"); return 0;
}
