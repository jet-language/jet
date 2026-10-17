/* Real Jet provider only, --wrap=jet_rt_alloc/jet_rt_free. Private UiABI
 * carriers, never a Rust/source-nominal allocation or C provider mock.
 * Compose e1306aee0 + c39d89b6b + 9e344d0f0 before native linking. */
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
typedef int64_t H;
typedef struct { unsigned char *data; uint64_t len, cap; } Text;
extern void jet_rt_alloc_init(void);
extern H jet_rt_alloc(H,H);
extern H __real_jet_rt_alloc(H,H);
extern void __real_jet_rt_free(H,H,H);
extern void jet_rt_free(H,H,H);
extern Text *jet_rt_string_from_static(const void *,H);
extern void jet_rt_string_drop(Text *);
#define DECLARE(T) extern H jet_rt_handle_clone_##T(H); extern void jet_rt_handle_drop_##T(H); extern bool jet_rt_handle_eq_##T(H,H)
DECLARE(JetUiCapabilityFacts); DECLARE(JetUiFileFilter); DECLARE(JetWebTableSort);
DECLARE(JetWebTableStatus); DECLARE(JetWebTableState); DECLARE(JetWebFormLifecycle);
DECLARE(JetWebMutationState); DECLARE(JetWebQueryState); DECLARE(JetWebStoreEvent);
extern void jet_rt_handle_field_jet_ui_file_filter_label(H,Text *);
extern H jet_rt_record_seq_list_rec_jet_ui_file_filter_new(H);
extern H jet_rt_record_seq_list_rec_jet_ui_file_filter_len(H);
extern H jet_rt_record_seq_list_rec_jet_ui_file_filter_get(H,H);
extern void jet_rt_record_seq_list_rec_jet_ui_file_filter_push(H,H);
extern H jet_rt_record_new_jet_web_table_sort(Text *,H);
extern Text *jet_rt_record_get_jet_web_table_sort_column(H);
extern H jet_rt_record_get_jet_web_table_sort_direction(H);
extern void jet_rt_handle_field_jet_web_form_lifecycle_result(H,Text *);
extern void jet_rt_handle_field_jet_web_form_lifecycle_error(H,Text *);
extern void jet_rt_handle_field_jet_web_mutation_state_value(H,Text *);
extern void jet_rt_handle_field_jet_web_mutation_state_result(H,Text *);
extern void jet_rt_handle_field_jet_web_mutation_state_error(H,Text *);
extern void jet_rt_handle_field_jet_web_mutation_state_context(H,Text *);
extern H jet_rt_handle_field_jet_web_mutation_state_optimistic(H);
extern H jet_rt_handle_field_jet_web_mutation_state_rollback(H);
extern H jet_rt_handle_field_jet_web_mutation_state_paused(H);
extern void jet_rt_handle_field_jet_web_query_state_value(H,Text *);
extern void jet_rt_handle_field_jet_web_query_state_error(H,Text *);
extern H jet_rt_handle_field_jet_web_query_state_offline(H);
extern void jet_rt_handle_field_jet_web_store_event_action(H,Text *);
static struct { H address,bytes,align; bool live; } ledger[4096];
static size_t allocations,frees;
static bool tracking;
H __wrap_jet_rt_alloc(H bytes,H align) {
    H address=__real_jet_rt_alloc(bytes,align);
    if(tracking) {
        assert(address&&bytes>0&&allocations<4096&&(uint64_t)address%(uint64_t)align==0);
        for(size_t i=0;i<allocations;++i) assert(!ledger[i].live||ledger[i].address!=address);
        ledger[allocations].address=address; ledger[allocations].bytes=bytes; ledger[allocations].align=align; ledger[allocations++].live=true;
    }
    return address;
}
void __wrap_jet_rt_free(H address,H bytes,H align) {
    if(tracking) {
        for(size_t i=allocations;i>0;--i) if(ledger[i-1].address==address&&ledger[i-1].live) {
            assert(ledger[i-1].bytes==bytes&&ledger[i-1].align==align); ledger[i-1].live=false; ++frees;
            __real_jet_rt_free(address,bytes,align); return;
        }
        assert(!"untracked or double free");
    }
    __real_jet_rt_free(address,bytes,align);
}
static uint64_t *w(H h) { return (uint64_t *)(uintptr_t)h; }
static H box(size_t bytes) { H h=jet_rt_alloc((H)bytes,8); memset((void *)(uintptr_t)h,0,bytes); return h; }
static H text(const char *s) { return (H)(uintptr_t)jet_rt_string_from_static(s,(H)strlen(s)); }
static void header_is(Text *view,H owner) { Text *t=(void *)(uintptr_t)owner; assert(view->data==t->data&&view->len==t->len&&view->cap==t->cap); }
static H texts(bool empty) {
    H h=box(24); if(empty) return h;
    H data=box(3*8); w(h)[0]=w(h)[1]=3; w(h)[2]=(uint64_t)data;
    w(data)[0]=(uint64_t)text("first"); w(data)[1]=(uint64_t)text("second"); w(data)[2]=(uint64_t)text(""); return h;
}
static void texts_independent(H a,H b) {
    assert(a!=b&&w(a)[0]==w(b)[0]); if(!w(a)[0]) return;
    assert(w(a)[2]!=w(b)[2]); for(uint64_t i=0;i<w(a)[0];++i) assert(w((H)w(a)[2])[i]!=w((H)w(b)[2])[i]);
}
static H file_filter(unsigned n) { H h=box(24); w(h)[0]=(uint64_t)text(n&1?"odd":"even"); w(h)[1]=(uint64_t)texts(false); w(h)[2]=(uint64_t)texts(n&1); return h; }
static void filters(void) {
    H list=jet_rt_record_seq_list_rec_jet_ui_file_filter_new(0);
    for(unsigned i=0;i<11;++i) jet_rt_record_seq_list_rec_jet_ui_file_filter_push(list,file_filter(i));
    assert(jet_rt_record_seq_list_rec_jet_ui_file_filter_len(list)==11&&w(list)[1]>=11);
    H a=jet_rt_record_seq_list_rec_jet_ui_file_filter_get(list,0),b=jet_rt_handle_clone_jet_ui_file_filter(a);
    assert(jet_rt_handle_eq_jet_ui_file_filter(a,b)&&w(a)[0]!=w(b)[0]);
    texts_independent((H)w(a)[1],(H)w(b)[1]); texts_independent((H)w(a)[2],(H)w(b)[2]);
    for(unsigned slot=1;slot<=2;++slot) {
        uint64_t *items=w((H)w((H)w(b)[slot])[2]),saved=items[0]; items[0]=items[1]; items[1]=saved;
        assert(!jet_rt_handle_eq_jet_ui_file_filter(a,b)); saved=items[0]; items[0]=items[1]; items[1]=saved;
    }
    Text view; jet_rt_handle_field_jet_ui_file_filter_label(b,&view); header_is(&view,(H)w(b)[0]);
    for(H i=0;i<11;++i) jet_rt_handle_drop_jet_ui_file_filter(jet_rt_record_seq_list_rec_jet_ui_file_filter_get(list,i));
    jet_rt_free((H)w(list)[2],(H)w(list)[1]*8,8); jet_rt_free(list,24,8);
    assert(view.len==4&&memcmp(view.data,"even",4)==0); jet_rt_handle_drop_jet_ui_file_filter(b); jet_rt_handle_drop_jet_ui_file_filter(0);
}
static void tables(void) {
    Text column={(unsigned char *)"column",6,0};
    for(H dir=0;dir<2;++dir) {
        H a=jet_rt_record_new_jet_web_table_sort(&column,dir),b=jet_rt_handle_clone_jet_web_table_sort(a);
        Text *got=jet_rt_record_get_jet_web_table_sort_column(a); assert(got!=(Text *)(uintptr_t)w(a)[0]&&got->len==6&&!memcmp(got->data,"column",6));
        assert(jet_rt_record_get_jet_web_table_sort_direction(a)==dir&&jet_rt_handle_eq_jet_web_table_sort(a,b));
        w(b)[1]^=1; assert(!jet_rt_handle_eq_jet_web_table_sort(a,b)); jet_rt_string_drop(got); jet_rt_handle_drop_jet_web_table_sort(a); jet_rt_handle_drop_jet_web_table_sort(b);
    }
    for(unsigned mask=0;mask<16;++mask) {
        H a=box(64); if(mask&1) w(a)[0]=(uint64_t)jet_rt_record_new_jet_web_table_sort(&column,1);
        if(mask&2) { H f=box(16); w(f)[0]=(uint64_t)text("filter"); w(f)[1]=(uint64_t)text("value"); w(a)[1]=(uint64_t)f; }
        w(a)[2]=UINT64_C(0x8000000000000000); w(a)[3]=UINT64_MAX; w(a)[4]=(uint64_t)texts(false);
        if(mask&4) w(a)[5]=(uint64_t)text("");
        if(mask&8) w(a)[6]=(uint64_t)text("");
        w(a)[7]=1;
        H b=jet_rt_handle_clone_jet_web_table_state(a); assert(jet_rt_handle_eq_jet_web_table_state(a,b));
        texts_independent((H)w(a)[4],(H)w(b)[4]);
        if(mask&1) assert(w(a)[0]!=w(b)[0]&&w((H)w(a)[0])[0]!=w((H)w(b)[0])[0]);
        if(mask&2) assert(w(a)[1]!=w(b)[1]&&w((H)w(a)[1])[1]!=w((H)w(b)[1])[1]);
        for(unsigned slot=0;slot<8;++slot) {
            uint64_t saved=w(b)[slot];
            if(slot==0||slot==1) { if(!saved) continue; w(b)[slot]=0; }
            else if(slot==4) { uint64_t *items=w((H)w((H)saved)[2]),v=items[0]; items[0]=items[1]; items[1]=v; assert(!jet_rt_handle_eq_jet_web_table_state(a,b)); v=items[0];items[0]=items[1];items[1]=v;continue; }
            else if(slot==5||slot==6) w(b)[slot]=saved?0:(uint64_t)text("");
            else w(b)[slot]^=1;
            assert(!jet_rt_handle_eq_jet_web_table_state(a,b)); if((slot==5||slot==6)&&!saved) jet_rt_string_drop((Text *)(uintptr_t)w(b)[slot]); w(b)[slot]=saved;
        }
        jet_rt_handle_drop_jet_web_table_state(a); jet_rt_handle_drop_jet_web_table_state(b);
    }
    for(H ordinal=0;ordinal<4;++ordinal) { H a=box(8);w(a)[0]=(uint64_t)ordinal;H b=jet_rt_handle_clone_jet_web_table_status(a);assert(jet_rt_handle_eq_jet_web_table_status(a,b));w(b)[0]^=1;assert(!jet_rt_handle_eq_jet_web_table_status(a,b));jet_rt_handle_drop_jet_web_table_status(a);jet_rt_handle_drop_jet_web_table_status(b); }
    jet_rt_handle_drop_jet_web_table_sort(0); jet_rt_handle_drop_jet_web_table_status(0); jet_rt_handle_drop_jet_web_table_state(0);
}
static void snapshot(H (*clone)(H),void (*drop)(H),bool (*eq)(H,H),unsigned count,uint64_t text_mask,uint64_t bool_mask,void (**text_fields)(H,Text *),H (**bool_fields)(H)) {
    H a=box(count*8);unsigned ti=0,bi=0;
    for(unsigned i=0;i<count;++i) w(a)[i]=(text_mask&(UINT64_C(1)<<i))?(uint64_t)text(i&1?"alpha":"beta"):(bool_mask&(UINT64_C(1)<<i))?(UINT64_C(0xbeef000000)|(i&1)):(i&1?UINT64_MAX:UINT64_C(0x8000000000000000));
    w(a)[0]=0; /* admitted status ordinal; counters retain their high bits */
    H b=clone(a);assert(eq(a,b));
    for(unsigned i=0;i<count;++i) {
        uint64_t saved=w(b)[i];
        if(text_mask&(UINT64_C(1)<<i)) { assert(saved!=w(a)[i]); Text view; text_fields[ti++](b,&view); header_is(&view,(H)saved); w(b)[i]=(uint64_t)text("different");assert(!eq(a,b));jet_rt_string_drop((Text *)(uintptr_t)w(b)[i]); }
        else if(bool_mask&(UINT64_C(1)<<i)) { assert(bool_fields[bi](b)==b+8*i); *(unsigned char *)(uintptr_t)bool_fields[bi++](b)^=1;assert(!eq(a,b)); }
        else { w(b)[i]^=1;assert(!eq(a,b)); }
        w(b)[i]=saved;
    }
    drop(a);assert(eq(b,b));drop(b);drop(0);
}
static void snapshots(void) {
    void (*life[])(H,Text *)={jet_rt_handle_field_jet_web_form_lifecycle_result,jet_rt_handle_field_jet_web_form_lifecycle_error};
    snapshot(jet_rt_handle_clone_jet_web_form_lifecycle,jet_rt_handle_drop_jet_web_form_lifecycle,jet_rt_handle_eq_jet_web_form_lifecycle,4,6,0,life,NULL);
    void (*mutation[])(H,Text *)={jet_rt_handle_field_jet_web_mutation_state_value,jet_rt_handle_field_jet_web_mutation_state_result,jet_rt_handle_field_jet_web_mutation_state_error,jet_rt_handle_field_jet_web_mutation_state_context};
    H (*mbools[])(H)={jet_rt_handle_field_jet_web_mutation_state_optimistic,jet_rt_handle_field_jet_web_mutation_state_rollback,jet_rt_handle_field_jet_web_mutation_state_paused};
    snapshot(jet_rt_handle_clone_jet_web_mutation_state,jet_rt_handle_drop_jet_web_mutation_state,jet_rt_handle_eq_jet_web_mutation_state,11,120,388,mutation,mbools);
    void (*query[])(H,Text *)={jet_rt_handle_field_jet_web_query_state_value,jet_rt_handle_field_jet_web_query_state_error};H (*qbools[])(H)={jet_rt_handle_field_jet_web_query_state_offline};
    snapshot(jet_rt_handle_clone_jet_web_query_state,jet_rt_handle_drop_jet_web_query_state,jet_rt_handle_eq_jet_web_query_state,6,6,32,query,qbools);
    H a=box(48);w(a)[0]=UINT64_MAX;w(a)[1]=UINT64_C(0x8000000000000000);w(a)[2]=(uint64_t)text("action");w(a)[3]=(uint64_t)texts(false);w(a)[4]=UINT64_MAX;w(a)[5]=2;
    H b=jet_rt_handle_clone_jet_web_store_event(a);assert(jet_rt_handle_eq_jet_web_store_event(a,b));texts_independent((H)w(a)[3],(H)w(b)[3]);
    for(unsigned i=0;i<6;++i) { uint64_t saved=w(b)[i]; if(i==2){w(b)[i]=(uint64_t)text("other");}else if(i==3){w((H)saved)[0]--;assert(!jet_rt_handle_eq_jet_web_store_event(a,b));w((H)saved)[0]++;continue;}else w(b)[i]^=1;assert(!jet_rt_handle_eq_jet_web_store_event(a,b));if(i==2)jet_rt_string_drop((Text *)(uintptr_t)w(b)[i]);w(b)[i]=saved; }
    Text view;jet_rt_handle_field_jet_web_store_event_action(b,&view);header_is(&view,(H)w(b)[2]);jet_rt_handle_drop_jet_web_store_event(a);assert(view.len==6&&!memcmp(view.data,"action",6));jet_rt_handle_drop_jet_web_store_event(b);jet_rt_handle_drop_jet_web_store_event(0);
    a=box(8);w(a)[0]=UINT64_C(0xabcdef00ff);b=jet_rt_handle_clone_jet_ui_capability_facts(a);assert(w(b)[0]==255&&jet_rt_handle_eq_jet_ui_capability_facts(a,b));w(b)[0]^=128;assert(!jet_rt_handle_eq_jet_ui_capability_facts(a,b));jet_rt_handle_drop_jet_ui_capability_facts(a);jet_rt_handle_drop_jet_ui_capability_facts(b);jet_rt_handle_drop_jet_ui_capability_facts(0);
}
int main(int argc,char **argv) {
    jet_rt_alloc_init();
    if(argc==2) {
        if(!strcmp(argv[1],"sort-tag")){Text column={(unsigned char *)"column",6,0};jet_rt_record_new_jet_web_table_sort(&column,2);}
        else if(!strcmp(argv[1],"negative-capacity"))jet_rt_record_seq_list_rec_jet_ui_file_filter_new(-1);
        else if(!strcmp(argv[1],"oversize-capacity"))jet_rt_record_seq_list_rec_jet_ui_file_filter_new(INT64_MAX);
        else if(!strcmp(argv[1],"index-zero"))jet_rt_record_seq_list_rec_jet_ui_file_filter_get(jet_rt_record_seq_list_rec_jet_ui_file_filter_new(0),0);
        else if(!strcmp(argv[1],"index-negative"))jet_rt_record_seq_list_rec_jet_ui_file_filter_get(jet_rt_record_seq_list_rec_jet_ui_file_filter_new(0),-1);
        else return 2;
        return 99;
    }
    tracking=true;filters();tables();snapshots();assert(allocations==frees);for(size_t i=0;i<allocations;++i)assert(!ledger[i].live);
    puts("UI slice3: ordered lists, optional table fields, raw counters, borrowed snapshots, byte masks and exact-once frees ok");return 0;
}
