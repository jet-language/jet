// Link only against the native Jet runtime + canonical ProgramAllocator facts.
// No Rust boxes, synthetic allocator or three-field MIR Err substitute.
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef intptr_t W;
typedef struct { char *data; W length, capacity; } Text;
typedef struct { W length, capacity; void *data; } List;
typedef struct { Text *text, *file; W line; } Context;
typedef struct { Text *source, *target; } Conversion;
typedef struct { Text *name, *value; } Field;
typedef struct { Text *variant; List fields; W has_span, start, end; } Details;
typedef struct Err { Text *message, *code; struct Err *cause; Text *identity; List context, conversions; Details *details; } Err;
_Static_assert(sizeof(Err)==88 && sizeof(Details)==56 && sizeof(Context)==24 && sizeof(Conversion)==16 && sizeof(Field)==16,"canonical native error ABI");
extern void *jet_rt_alloc(W,W);
extern Text *jet_rt_string_from_static(const char *,W);
extern void jet_rt_string_drop(Text *);
extern Err *jet_rt_record_new_JetErr(Text *,Text **,Err *);
extern Err *jet_err_from_message(Text *);
extern Err *jet_rt_handle_clone_JetErr(Err *);
extern void jet_rt_handle_drop_JetErr(Err *);
extern bool jet_rt_handle_eq_JetErr(Err *,Err *);
extern Text *jet_rt_record_get_JetErr_message(Err *);
extern W jet_rt_record_get_JetErr_code(Err *,Text **);
extern W jet_rt_record_get_JetErr_cause(Err *,Err **);
extern Text *jet_rt_handle_display_JetErr(Err *);
extern Text *jet_rt_handle_show_JetErr(Err *);
extern Text *jet_rt_handle_debug_JetErr(Err *);
extern Err *jet_err_apply_conversion(Err *,Text *,Text *);
extern Text *jet_render_err(Err *);
extern Text *jet_error_report_json(Err *,void *);
extern void jet_rt_program_allocator_configure(W,W);
extern void jet_rt_program_allocator_restore(W,W);
extern W jet_rt_program_allocator_fact(W);
static Text *text(const char *s) { return jet_rt_string_from_static(s,(W)strlen(s)); }
static void equal(Text *value,const char *expected) {
    assert(value->length==(W)strlen(expected));
    assert(!memcmp(value->data,expected,(size_t)value->length));
    jet_rt_string_drop(value);
}
int main(void) {
    // Pack constructors initialize allocator/thread roots before measuring.
    W cap=jet_rt_program_allocator_fact(4), mode=jet_rt_program_allocator_fact(5);
    jet_rt_program_allocator_configure(1,0);
    Text *message=text("outer"), *code=text("E42"), *cause_message=text("inner");
    Err *cause=jet_err_from_message(cause_message);
    jet_rt_string_drop(cause_message);
    Err *value=jet_rt_record_new_JetErr(message,&code,cause); // cause TAKEN
    assert(value->cause==cause && value->message!=message && value->code!=code);
    jet_rt_string_drop(message); jet_rt_string_drop(code);
    Text *source=text("Source"), *target=text("Target");
    Err *converted=jet_err_apply_conversion(value,source,target);
    jet_rt_string_drop(source); jet_rt_string_drop(target); jet_rt_handle_drop_JetErr(value);
    Context *context=jet_rt_alloc(sizeof(*context),8);
    *context=(Context){text("note\n\t\"\\\177\302\200"),text("main.jet"),7};
    converted->context=(List){1,1,context};
    Details *details=jet_rt_alloc(sizeof(*details),8);
    Field *field=jet_rt_alloc(sizeof(*field),8);
    *field=(Field){text("n"),text("[1,{\"x\":true}]")};
    *details=(Details){text("Variant"),{1,1,field},1,10,20};
    converted->details=details;
    Err *clone=jet_rt_handle_clone_JetErr(converted);
    assert(clone!=converted && clone->cause!=converted->cause && clone->identity!=converted->identity);
    assert(clone->context.data!=converted->context.data && clone->conversions.data!=converted->conversions.data);
    assert(clone->details!=details && clone->details->fields.data!=details->fields.data);
    assert(jet_rt_handle_eq_JetErr(converted,clone));
    jet_rt_handle_drop_JetErr(converted);
    equal(jet_rt_record_get_JetErr_message(clone),"outer");
    Text *projection=NULL; assert(jet_rt_record_get_JetErr_code(clone,&projection)==1); equal(projection,"E42");
    Err *projected_cause=NULL; assert(jet_rt_record_get_JetErr_cause(clone,&projected_cause)==1);
    assert(projected_cause!=clone->cause); equal(jet_rt_record_get_JetErr_message(projected_cause),"inner");
    jet_rt_handle_drop_JetErr(projected_cause);
    equal(jet_rt_handle_display_JetErr(clone),"outer");
    equal(jet_rt_handle_show_JetErr(clone),"outer");
    equal(jet_rt_handle_debug_JetErr(clone),"outer");
    equal(jet_render_err(clone),"Error [E42]: outer (type: Source)\n  cause: inner\n  context (main.jet:7): note\n\t\"\\\177\302\200\n  conversion: Source -> Target");
    equal(jet_error_report_json(clone,NULL),"{\"schema\":\"jet.err/v1\",\"message\":\"outer\",\"code\":\"E42\",\"cause\":{\"schema\":\"jet.err/v1\",\"message\":\"inner\",\"code\":null,\"cause\":null},\"typed_identity\":\"Source\",\"context_frames\":[{\"text\":\"note\\n\\t\\\"\\\\\\u007f\\u0080\",\"file\":\"main.jet\",\"line\":7}],\"conversion_history\":[{\"source\":\"Source\",\"target\":\"Target\"}],\"details\":{\"variant\":\"Variant\",\"fields\":{\"n\":[1,{\"x\":true}]},\"source_span\":{\"start\":10,\"end\":20}}}");
    jet_rt_handle_drop_JetErr(clone);
    // Empty code differs from None; absent projections never initialize slots.
    message=text(""); code=text(""); value=jet_rt_record_new_JetErr(message,&code,NULL);
    jet_rt_string_drop(message); jet_rt_string_drop(code);
    equal(jet_render_err(value),"Error []: ");
    projected_cause=NULL; assert(!jet_rt_record_get_JetErr_cause(value,&projected_cause) && !projected_cause);
    jet_rt_handle_drop_JetErr(value);
    assert(jet_rt_program_allocator_fact(2)==0); // LIVE BYTES, not allocation history
    jet_rt_program_allocator_restore(mode,cap);
    puts("Err88: taken cause, deep clone/projection/metadata, root+wire bytes, ledger zero");
    return 0;
}
