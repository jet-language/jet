// Real Jet-object client, link -Wl,--wrap=jet_rt_free. GeometryABI only.
// IEEE derive truth tables: LinalgFns561-584, MathTaskMem83-93.
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { double x,y; int64_t frame; } Coord;
typedef struct { double lanes[6]; int64_t from,to; } Transform;
typedef struct { Coord *origin,*direction; } Ray;
_Static_assert(sizeof(Coord)==24 && offsetof(Coord,frame)==16,"Coord24");
_Static_assert(sizeof(Transform)==64 && offsetof(Transform,from)==48,"Transform64");
_Static_assert(sizeof(Ray)==16 && offsetof(Ray,direction)==8,"Ray16");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern int64_t jet_int_owned_from_i64(int64_t);
extern int64_t jet_int_owned_to_i64(int64_t);
extern int64_t jet_int_add(int64_t,int64_t);
extern void jet_rt_int_drop(int64_t);
extern Coord *jet_rt_record_new_jet_coord2(double,double,int64_t);
extern Coord *jet_rt_handle_clone_jet_coord2(Coord *);
extern void jet_rt_handle_drop_jet_coord2(Coord *);
extern bool jet_rt_handle_eq_jet_coord2(Coord *,Coord *);
extern double jet_rt_record_get_jet_coord2_x(Coord *);
extern double jet_rt_record_get_jet_coord2_y(Coord *);
extern int64_t jet_rt_record_get_jet_coord2_frame_id(Coord *);
extern Transform *jet_rt_record_new_jet_transform2(double,double,double,double,double,double,int64_t,int64_t);
extern Transform *jet_rt_handle_clone_jet_transform2(Transform *);
extern void jet_rt_handle_drop_jet_transform2(Transform *);
extern bool jet_rt_handle_eq_jet_transform2(Transform *,Transform *);
extern double jet_rt_record_get_jet_transform2_m00(Transform *);
extern double jet_rt_record_get_jet_transform2_m01(Transform *);
extern double jet_rt_record_get_jet_transform2_m10(Transform *);
extern double jet_rt_record_get_jet_transform2_m11(Transform *);
extern double jet_rt_record_get_jet_transform2_tx(Transform *);
extern double jet_rt_record_get_jet_transform2_ty(Transform *);
extern int64_t jet_rt_record_get_jet_transform2_from_frame(Transform *);
extern int64_t jet_rt_record_get_jet_transform2_to_frame(Transform *);
extern Ray *jet_rt_record_new_jet_ray2(Coord *,Coord *);
extern Ray *jet_rt_handle_clone_jet_ray2(Ray *);
extern void jet_rt_handle_drop_jet_ray2(Ray *);
extern bool jet_rt_handle_eq_jet_ray2(Ray *,Ray *);
extern Coord *jet_rt_record_get_jet_ray2_origin(Ray *);
extern Coord *jet_rt_record_get_jet_ray2_direction(Ray *);
#define ARRAY_API(name) \
extern double *jet_rt_handle_clone_##name(double *); \
extern void jet_rt_handle_drop_##name(double *); \
extern bool jet_rt_handle_eq_##name(double *,double *)
ARRAY_API(Vec2); ARRAY_API(Vec3); ARRAY_API(Vec4); ARRAY_API(Mat3); ARRAY_API(Mat4);
static struct { void *pointer; int64_t bytes; unsigned frees; } watched[16];
static size_t count;
static unsigned intercepted;
void __wrap_jet_rt_free(void *p,int64_t bytes,int64_t align) {
    ++intercepted;
    for(size_t i=0;i<count;++i) if(watched[i].pointer==p) {
        assert(bytes==watched[i].bytes && align==8); assert(++watched[i].frees==1);
    }
    __real_jet_rt_free(p,bytes,align);
}
static void watch(void *p,int64_t bytes) {
    assert(p && count<16);
    for(size_t i=0;i<count;++i) assert(watched[i].pointer!=p);
    watched[count].pointer=p; watched[count].bytes=bytes; watched[count++].frees=0;
}
static void done(void) { assert(intercepted>0); for(size_t i=0;i<count;++i) assert(watched[i].frees==1); count=0; }
static double nan_value(void) { uint64_t bits=UINT64_C(0x7ff8000000000123); double d; memcpy(&d,&bits,8); return d; }
static void arrays(size_t n,double *(*clone)(double *),void (*drop)(double *),bool (*eq)(double *,double *)) {
    double *a=jet_rt_alloc((int64_t)n*8,8); for(size_t i=0;i<n;++i) a[i]=(double)i+1;
    double *b=clone(a); assert(a!=b && memcmp(a,b,n*8)==0 && eq(a,b));
    for(size_t i=0;i<n;++i) {
        b[i]+=1; assert(!eq(a,b)); b[i]-=1;
        double saved=a[i]; a[i]=nan_value(); b[i]=a[i];
        assert(!eq(a,a) && !eq(a,b)); a[i]=saved; b[i]=saved;
        a[i]=-0.0; b[i]=0.0; assert(eq(a,b)); a[i]=saved; b[i]=saved;
    }
    watch(a,(int64_t)n*8); watch(b,(int64_t)n*8); drop(a); drop(b); done(); drop(NULL);
}
static void coords(void) {
    int64_t hi=jet_int_owned_from_i64(INT64_MAX);
    Coord *a=jet_rt_record_new_jet_coord2(1,2,hi), *b=jet_rt_handle_clone_jet_coord2(a);
    int64_t first=jet_rt_record_get_jet_coord2_frame_id(a), second=jet_rt_record_get_jet_coord2_frame_id(a);
    assert(a->frame==INT64_MAX && first!=hi && second!=hi && first!=second);
    jet_rt_int_drop(hi);
    assert(jet_int_owned_to_i64(first)==INT64_MAX && jet_int_owned_to_i64(second)==INT64_MAX);
    jet_rt_int_drop(first); jet_rt_int_drop(second);
    assert(jet_rt_record_get_jet_coord2_x(a)==1 && jet_rt_record_get_jet_coord2_y(a)==2);
    assert(a!=b && jet_rt_handle_eq_jet_coord2(a,b));
    b->frame^=1; assert(!jet_rt_handle_eq_jet_coord2(a,b)); b->frame^=1;
    double *af[]={&a->x,&a->y}, *bf[]={&b->x,&b->y};
    for(size_t i=0;i<2;++i) {
        *bf[i]+=1; assert(!jet_rt_handle_eq_jet_coord2(a,b)); *bf[i]-=1;
        double saved=*af[i]; *af[i]=nan_value(); *bf[i]=*af[i];
        assert(!jet_rt_handle_eq_jet_coord2(a,a) && !jet_rt_handle_eq_jet_coord2(a,b));
        *af[i]=-0.0; *bf[i]=0.0; assert(jet_rt_handle_eq_jet_coord2(a,b)); *af[i]=saved; *bf[i]=saved;
    }
    watch(a,24); watch(b,24); jet_rt_handle_drop_jet_coord2(a); jet_rt_handle_drop_jet_coord2(b); done();
    jet_rt_handle_drop_jet_coord2(NULL);
}
static void transforms(void) {
    int64_t hi=jet_int_owned_from_i64(INT64_MAX),lo=jet_int_owned_from_i64(INT64_MIN);
    Transform *a=jet_rt_record_new_jet_transform2(1,2,3,4,5,6,hi,lo), *b=jet_rt_handle_clone_jet_transform2(a);
    int64_t from=jet_rt_record_get_jet_transform2_from_frame(a),to=jet_rt_record_get_jet_transform2_to_frame(a);
    assert(a->from==INT64_MAX && a->to==INT64_MIN && from!=hi && to!=lo);
    jet_rt_int_drop(hi); jet_rt_int_drop(lo);
    assert(jet_int_owned_to_i64(from)==INT64_MAX && jet_int_owned_to_i64(to)==INT64_MIN);
    jet_rt_int_drop(from); jet_rt_int_drop(to);
    assert(a!=b && jet_rt_handle_eq_jet_transform2(a,b));
    double (*getters[])(Transform *)={jet_rt_record_get_jet_transform2_m00,jet_rt_record_get_jet_transform2_m01,jet_rt_record_get_jet_transform2_m10,jet_rt_record_get_jet_transform2_m11,jet_rt_record_get_jet_transform2_tx,jet_rt_record_get_jet_transform2_ty};
    for(size_t i=0;i<6;++i) {
        assert(getters[i](a)==(double)i+1);
        b->lanes[i]+=1; assert(!jet_rt_handle_eq_jet_transform2(a,b)); b->lanes[i]-=1;
        double saved=a->lanes[i]; a->lanes[i]=nan_value(); b->lanes[i]=a->lanes[i];
        assert(!jet_rt_handle_eq_jet_transform2(a,a) && !jet_rt_handle_eq_jet_transform2(a,b));
        a->lanes[i]=-0.0; b->lanes[i]=0.0; assert(jet_rt_handle_eq_jet_transform2(a,b)); a->lanes[i]=saved; b->lanes[i]=saved;
    }
    b->from^=1; assert(!jet_rt_handle_eq_jet_transform2(a,b)); b->from^=1;
    b->to^=1; assert(!jet_rt_handle_eq_jet_transform2(a,b)); b->to^=1;
    watch(a,64); watch(b,64); jet_rt_handle_drop_jet_transform2(a); jet_rt_handle_drop_jet_transform2(b); done();
    jet_rt_handle_drop_jet_transform2(NULL);
}
static void rays(void) {
    Coord *o=jet_rt_record_new_jet_coord2(1,2,3), *d=jet_rt_record_new_jet_coord2(4,5,6);
    Ray *a=jet_rt_record_new_jet_ray2(o,d), *b=jet_rt_handle_clone_jet_ray2(a);
    assert(a->origin==o && a->direction==d && a!=b && b->origin!=o && b->direction!=d);
    assert(jet_rt_handle_eq_jet_ray2(a,b));
    Coord *go=jet_rt_record_get_jet_ray2_origin(a), *gd=jet_rt_record_get_jet_ray2_direction(a);
    assert(go!=o && go!=b->origin && gd!=d && gd!=b->direction);
    b->origin->x+=1; assert(!jet_rt_handle_eq_jet_ray2(a,b)); b->origin->x-=1;
    b->direction->frame^=1; assert(!jet_rt_handle_eq_jet_ray2(a,b)); b->direction->frame^=1;
    b->direction->y=nan_value(); assert(!jet_rt_handle_eq_jet_ray2(b,b)); b->direction->y=d->y;
    watch(a,16); watch(o,24); watch(d,24); watch(b,16); watch(b->origin,24); watch(b->direction,24);
    jet_rt_handle_drop_jet_ray2(a); jet_rt_handle_drop_jet_ray2(b); done();
    assert(go->x==1 && gd->frame==6); watch(go,24); watch(gd,24);
    jet_rt_handle_drop_jet_coord2(go); jet_rt_handle_drop_jet_coord2(gd); done(); jet_rt_handle_drop_jet_ray2(NULL);
}
int main(int argc,char **argv) {
    jet_rt_alloc_init();
    if(argc>1) {
        int64_t hi=jet_int_owned_from_i64(INT64_MAX),over=jet_int_add(hi,1);
        if(strcmp(argv[1],"coord-overflow")==0) (void)jet_rt_record_new_jet_coord2(1,2,over);
        else if(strcmp(argv[1],"transform-overflow")==0) (void)jet_rt_record_new_jet_transform2(1,2,3,4,5,6,0,over);
        else return 2;
        assert(!"overflow mode returned"); return 1;
    }
    arrays(2,jet_rt_handle_clone_vec2,jet_rt_handle_drop_vec2,jet_rt_handle_eq_vec2);
    arrays(3,jet_rt_handle_clone_vec3,jet_rt_handle_drop_vec3,jet_rt_handle_eq_vec3);
    arrays(4,jet_rt_handle_clone_vec4,jet_rt_handle_drop_vec4,jet_rt_handle_eq_vec4);
    arrays(9,jet_rt_handle_clone_mat3,jet_rt_handle_drop_mat3,jet_rt_handle_eq_mat3);
    arrays(16,jet_rt_handle_clone_mat4,jet_rt_handle_drop_mat4,jet_rt_handle_eq_mat4);
    coords(); transforms(); rays(); puts("GeometryGlue ownership/equality OK"); return 0;
}
