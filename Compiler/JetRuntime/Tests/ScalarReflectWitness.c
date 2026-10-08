// Real Jet-object client, link --wrap=jet_rt_free. Private native ABI only.
// Goldens: StructuralDebug91-120/310-319, CommonTypes6822-6828, Reflect56-69.
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { unsigned char *data; int64_t length,capacity; } Text;
typedef struct { int64_t start,end,exclusive; } Range;
typedef struct { int64_t seed,checked,failures; } Solver;
typedef struct Field Field;
typedef struct { int64_t length,capacity; Field **data; } List;
typedef struct { Text *type_name,*path,*display; List *fields; } Value;
struct Field { Text *name; Value *value; };
_Static_assert(sizeof(Range)==24 && sizeof(Solver)==24,"scalar ABI");
_Static_assert(sizeof(Value)==32 && offsetof(Value,fields)==24 && sizeof(Field)==16 && sizeof(List)==24,"reflection ABI");
extern void jet_rt_alloc_init(void);
extern void jet_rt_host_init(void);
extern void *jet_std_io_stdout(void);
extern int64_t jet_std_io_stdout_write(void *,Text *,void **);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern Text *jet_rt_string_from_static(const void *,int64_t);
extern void jet_rt_string_drop(Text *);
extern int64_t jet_int_owned_from_i64(int64_t);
extern int64_t jet_int_owned_to_i64(int64_t);
extern int64_t jet_int_add(int64_t,int64_t);
extern void jet_rt_int_drop(int64_t);
extern Range *jet_rt_record_new_JetRange(int64_t,int64_t,uint8_t);
extern Range *jet_rt_handle_clone_JetRange(Range *);
extern void jet_rt_handle_drop_JetRange(Range *);
extern bool jet_rt_handle_eq_JetRange(Range *,Range *);
extern int64_t jet_rt_record_get_JetRange_start(Range *);
extern int64_t jet_rt_record_get_JetRange_end(Range *);
extern bool jet_rt_record_get_JetRange_exclusive(Range *);
extern Text *jet_rt_handle_show_JetRange(Range *);
extern Text *jet_rt_handle_display_JetRange(Range *);
extern Text *jet_rt_handle_debug_JetRange(Range *);
extern Solver *jet_rt_record_new_Solver(int64_t,int64_t,int64_t);
extern Solver *jet_rt_handle_clone_Solver(Solver *);
extern void jet_rt_handle_drop_Solver(Solver *);
extern bool jet_rt_handle_eq_Solver(Solver *,Solver *);
extern int64_t jet_rt_record_get_Solver_seed(Solver *);
extern int64_t jet_rt_record_get_Solver_checked(Solver *);
extern int64_t jet_rt_record_get_Solver_failures(Solver *);
extern Text *jet_rt_handle_show_Solver(Solver *);
extern List *jet_rt_record_seq_List_Rec_JetReflectField_new(int64_t);
extern int64_t jet_rt_record_seq_List_Rec_JetReflectField_len(List *);
extern Field *jet_rt_record_seq_List_Rec_JetReflectField_get(List *,int64_t);
extern void jet_rt_record_seq_List_Rec_JetReflectField_push(List *,Field *);
extern Value *jet_rt_record_new_JetReflectValue(Text *,Text *,Text *,List *);
extern Value *jet_rt_handle_clone_JetReflectValue(Value *);
extern void jet_rt_handle_drop_JetReflectValue(Value *);
extern Text *jet_rt_record_get_JetReflectValue_type_name(Value *);
extern Text *jet_rt_record_get_JetReflectValue_path(Value *);
extern Text *jet_rt_record_get_JetReflectValue_display(Value *);
extern List *jet_rt_record_get_JetReflectValue_fields(Value *);
extern Text *jet_rt_handle_show_JetReflectValue(Value *);
extern Text *jet_rt_handle_display_JetReflectValue(Value *);
extern Field *jet_rt_record_new_JetReflectField(Text *,Value *);
extern Field *jet_rt_handle_clone_JetReflectField(Field *);
extern void jet_rt_handle_drop_JetReflectField(Field *);
extern Text *jet_rt_record_get_JetReflectField_name(Field *);
extern Value *jet_rt_record_get_JetReflectField_value(Field *);
extern Text *jet_rt_handle_show_JetReflectField(Field *);
static struct { void *pointer; int64_t bytes,align; unsigned frees; } watched[256];
static size_t count;
static unsigned intercepted;
void __wrap_jet_rt_free(void *p,int64_t bytes,int64_t align) {
    ++intercepted;
    for(size_t i=0;i<count;++i) if(watched[i].pointer==p) {
        assert(bytes==watched[i].bytes && align==watched[i].align); assert(++watched[i].frees==1);
    }
    __real_jet_rt_free(p,bytes,align);
}
static void watch(void *p,int64_t bytes,int64_t align) {
    assert(p && count<256); for(size_t i=0;i<count;++i) assert(watched[i].pointer!=p);
    watched[count].pointer=p; watched[count].bytes=bytes; watched[count].align=align; watched[count++].frees=0;
}
static void watch_text(Text *t) { watch(t,24,8); if(t->capacity) watch(t->data,t->capacity,1); }
static void done(void) { assert(intercepted>0); for(size_t i=0;i<count;++i) assert(watched[i].frees==1); count=0; }
static Text *text(const char *s) { return jet_rt_string_from_static(s,(int64_t)strlen(s)); }
static void text_is(Text *t,const char *s) { assert(t->length==(int64_t)strlen(s)); assert(memcmp(t->data,s,(size_t)t->length)==0); }
static void owned_text_is(Text *t,const char *s) { text_is(t,s); watch_text(t); jet_rt_string_drop(t); done(); }
static void owned_int_is(int64_t t,int64_t raw) { assert(jet_int_owned_to_i64(t)==raw); jet_rt_int_drop(t); }
static void ranges(void) {
    int64_t hi=jet_int_owned_from_i64(INT64_MAX),lo=jet_int_owned_from_i64(INT64_MIN);
    Range *a=jet_rt_record_new_JetRange(lo,hi,255), *b=jet_rt_handle_clone_JetRange(a);
    int64_t first=jet_rt_record_get_JetRange_start(a),second=jet_rt_record_get_JetRange_start(a);
    assert(first!=lo && second!=lo && first!=second); jet_rt_int_drop(lo); jet_rt_int_drop(hi);
    owned_int_is(first,INT64_MIN); owned_int_is(second,INT64_MIN);
    owned_int_is(jet_rt_record_get_JetRange_end(a),INT64_MAX);
    assert(a!=b && a->exclusive==1 && jet_rt_record_get_JetRange_exclusive(a) && jet_rt_handle_eq_JetRange(a,b));
    int64_t *slots[]={&b->start,&b->end,&b->exclusive};
    for(size_t i=0;i<3;++i) { *slots[i]^=1; assert(!jet_rt_handle_eq_JetRange(a,b)); *slots[i]^=1; }
    b->exclusive^=256; assert(jet_rt_handle_eq_JetRange(a,b));
    const char *golden="Range { start: -9223372036854775808, end: 9223372036854775807, exclusive: true }";
    owned_text_is(jet_rt_handle_show_JetRange(a),golden); owned_text_is(jet_rt_handle_display_JetRange(a),golden); owned_text_is(jet_rt_handle_debug_JetRange(a),golden);
    watch(a,24,8); watch(b,24,8); jet_rt_handle_drop_JetRange(a); jet_rt_handle_drop_JetRange(b); done();
    a=jet_rt_record_new_JetRange(9,-4,0); owned_text_is(jet_rt_handle_show_JetRange(a),"Range { start: 9, end: -4, exclusive: false }");
    watch(a,24,8); jet_rt_handle_drop_JetRange(a); done(); jet_rt_handle_drop_JetRange(NULL);
}
static void solvers(void) {
    int64_t hi=jet_int_owned_from_i64(INT64_MAX),lo=jet_int_owned_from_i64(INT64_MIN);
    Solver *a=jet_rt_record_new_Solver(lo,hi,lo), *b=jet_rt_handle_clone_Solver(a);
    int64_t first=jet_rt_record_get_Solver_seed(a),second=jet_rt_record_get_Solver_seed(a);
    assert(first!=lo && second!=lo && first!=second); jet_rt_int_drop(hi); jet_rt_int_drop(lo);
    owned_int_is(first,INT64_MIN); owned_int_is(second,INT64_MIN);
    owned_int_is(jet_rt_record_get_Solver_checked(a),INT64_MAX); owned_int_is(jet_rt_record_get_Solver_failures(a),INT64_MIN);
    assert(a!=b && jet_rt_handle_eq_Solver(a,b)); int64_t *slots[]={&b->seed,&b->checked,&b->failures};
    for(size_t i=0;i<3;++i) { *slots[i]^=1; assert(!jet_rt_handle_eq_Solver(a,b)); *slots[i]^=1; }
    owned_text_is(jet_rt_handle_show_Solver(a),"Solver { seed: -9223372036854775808, checked: 9223372036854775807, failures: -9223372036854775808 }");
    watch(a,24,8); watch(b,24,8); jet_rt_handle_drop_Solver(a); jet_rt_handle_drop_Solver(b); done(); jet_rt_handle_drop_Solver(NULL);
}
static Value *value(const char *type,const char *path,const char *display,List *list) {
    Text *t=text(type), *p=text(path), *d=text(display);
    Value *v=jet_rt_record_new_JetReflectValue(t,p,d,list);
    assert(v->type_name!=t && v->path!=p && v->display!=d && v->fields==list);
    jet_rt_string_drop(t); jet_rt_string_drop(p); jet_rt_string_drop(d); return v;
}
static Field *field(const char *name,Value *v) {
    Text *n=text(name); Field *f=jet_rt_record_new_JetReflectField(n,v);
    assert(f->name!=n && f->value==v); jet_rt_string_drop(n); return f;
}
static void watch_value(Value *v);
static void watch_field(Field *f) { watch(f,16,8); watch_text(f->name); watch_value(f->value); }
static void watch_value(Value *v) {
    watch(v,32,8); watch_text(v->type_name); watch_text(v->path); watch_text(v->display);
    List *l=v->fields; watch(l,24,8); if(l->capacity) watch(l->data,l->capacity*8,8);
    for(int64_t i=0;i<l->length;++i) watch_field(l->data[i]);
}
static void reflections(void) {
    List *empty=jet_rt_record_seq_List_Rec_JetReflectField_new(0);
    Value *leaf=value("Int","root.x","42",empty); Field *f=field("x",leaf);
    List *rows=jet_rt_record_seq_List_Rec_JetReflectField_new(1);
    jet_rt_record_seq_List_Rec_JetReflectField_push(rows,f);
    assert(jet_rt_record_seq_List_Rec_JetReflectField_len(rows)==1 && jet_rt_record_seq_List_Rec_JetReflectField_get(rows,0)==f);
    Field **old=rows->data; int64_t capacity=rows->capacity;
    Field *extra=field("other",value("Int","root.other","7",jet_rt_record_seq_List_Rec_JetReflectField_new(0)));
    watch(old,capacity*8,8); jet_rt_record_seq_List_Rec_JetReflectField_push(rows,extra); done(); // reserve frees old buffer exactly once
    Value *root=value("Record","root","{x: 42}",rows), *clone=jet_rt_handle_clone_JetReflectValue(root);
    assert(root!=clone && root->type_name!=clone->type_name && root->fields!=clone->fields && root->fields->data!=clone->fields->data);
    Field *copied=jet_rt_record_seq_List_Rec_JetReflectField_get(clone->fields,0);
    assert(copied!=f && copied->name!=f->name && copied->value!=leaf && copied->value->fields!=empty);
    copied->value->display->data[0]='9'; assert(leaf->display->data[0]=='4'); copied->value->display->data[0]='4';
    assert(jet_rt_record_get_JetReflectValue_fields(root)==rows);
    Text *tn=jet_rt_record_get_JetReflectValue_type_name(root), *p=jet_rt_record_get_JetReflectValue_path(root), *d=jet_rt_record_get_JetReflectValue_display(root), *name=jet_rt_record_get_JetReflectField_name(f);
    assert(tn!=root->type_name && p!=root->path && d!=root->display && name!=f->name);
    Value *got=jet_rt_record_get_JetReflectField_value(f); Field *fc=jet_rt_handle_clone_JetReflectField(f);
    assert(got!=leaf && got->display!=leaf->display && fc!=f && fc->value!=leaf);
    owned_text_is(jet_rt_handle_show_JetReflectValue(root),"Value(Record)");
    owned_text_is(jet_rt_handle_display_JetReflectValue(root),"{x: 42}");
    owned_text_is(jet_rt_handle_show_JetReflectField(f),"Field(x: 42)");
    watch_value(root); watch_value(clone); watch_value(got); watch_field(fc);
    jet_rt_handle_drop_JetReflectValue(root); jet_rt_handle_drop_JetReflectValue(clone); jet_rt_handle_drop_JetReflectValue(got); jet_rt_handle_drop_JetReflectField(fc); done();
    text_is(tn,"Record"); text_is(p,"root"); text_is(d,"{x: 42}"); text_is(name,"x");
    watch_text(tn); watch_text(p); watch_text(d); watch_text(name);
    jet_rt_string_drop(tn); jet_rt_string_drop(p); jet_rt_string_drop(d); jet_rt_string_drop(name); done();
    jet_rt_handle_drop_JetReflectValue(NULL); jet_rt_handle_drop_JetReflectField(NULL);
}
int main(int argc,char **argv) {
    jet_rt_alloc_init();
    if(argc>1) {
        if(strcmp(argv[1],"bounds-equal")==0 || strcmp(argv[1],"bounds-negative")==0) {
            List *list=jet_rt_record_seq_List_Rec_JetReflectField_new(0);
            jet_rt_host_init();
            Text *pending=text("flushed"); void *error=NULL;
            assert(jet_std_io_stdout_write(jet_std_io_stdout(),pending,&error)==1);
            jet_rt_string_drop(pending); // no explicit flush; bounds stop must flush
            (void)jet_rt_record_seq_List_Rec_JetReflectField_get(list,strcmp(argv[1],"bounds-negative")==0?-1:0);
        } else {
            int64_t hi=jet_int_owned_from_i64(INT64_MAX),over=jet_int_add(hi,1);
            if(strcmp(argv[1],"range-overflow")==0) (void)jet_rt_record_new_JetRange(0,over,0);
            else if(strcmp(argv[1],"solver-overflow")==0) (void)jet_rt_record_new_Solver(0,0,over);
            else return 2;
        }
        assert(!"stop mode returned"); return 1;
    }
    ranges(); solvers(); reflections(); puts("ScalarReflect ownership/equality/render OK"); return 0;
}
