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
typedef struct { Text *function, *file; W line, column; Text *note; W count; } Hop;
typedef struct { Hop *origin; List hops; } Journey;
_Static_assert(sizeof(Hop)==48 && sizeof(Journey)==32,"canonical native Journey ABI");
extern Text *jet_journey_render_styled(Journey *,bool,W);
extern Journey *jet_rt_native_journey_new(void);
extern Journey *jet_rt_native_journey_clone(Journey *);
extern void jet_rt_native_journey_drop(Journey *);
extern void jet_rt_native_journey_reset(Journey *);
extern void jet_rt_native_journey_origin(Journey *,Text *,W,W,Text *);
extern void jet_rt_native_journey_frame(Journey *,Text *,W,W,Text *,Text *);
extern Journey *jet_rt_native_journey_take(Journey **);
extern void jet_rt_native_journey_adopt(Journey *,Journey *);
extern W jet_rt_journey_destructor(W,Journey *);
extern Err *jet_rt_native_err_context_frame(Err *,Journey *,Text *,W,W,Text *,Text *);
extern void *jet_rt_alloc(W,W);
extern Text *jet_rt_string_from_static(const char *,W);
extern void jet_rt_string_drop(Text *);
extern Err *jet_rt_record_new_jet_err(Text *,Text **,Err *);
extern Err *jet_err_from_message(Text *);
extern Err *jet_rt_handle_clone_jet_err(Err *);
extern void jet_rt_handle_drop_jet_err(Err *);
extern bool jet_rt_handle_eq_jet_err(Err *,Err *);
extern Text *jet_rt_record_get_jet_err_message(Err *);
extern W jet_rt_record_get_jet_err_code(Err *,Text **);
extern W jet_rt_record_get_jet_err_cause(Err *,Err **);
extern Text *jet_rt_handle_display_jet_err(Err *);
extern Text *jet_rt_handle_show_jet_err(Err *);
extern Text *jet_rt_handle_debug_jet_err(Err *);
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
static Text borrowed(char *value) { return (Text){value,(W)strlen(value),0}; }
static void journey_goldens(void) {
    // Literal bytes from Outcome.rs:2066-2172; no regenerated approximation.
    Text file=borrowed("Examples/features/errors/error_context.jet");
    Text empty=borrowed(""), a=borrowed("read_raw"), b=borrowed("parse_config"), c=borrowed("load_config"), d=borrowed("run");
    Text n1=borrowed("reading raw config"), n2=borrowed("loading config app.toml");
    Hop origin={&a,&file,4,1,&empty,1}, first={&b,&file,7,1,&n1,1}, second={&c,&file,12,1,&n2,1}, third={&d,&file,16,1,&empty,1};
    Hop *hops[]={&first,&second,&third}; Journey journey={&origin,{3,3,hops}};
    const char *plain=
        "  failed here: read_raw (Examples/features/errors/error_context.jet:4)\n"
        " Trail [E3002] (3 hops via ?, origin first):\n"
        "  1. parse_config (Examples/features/errors/error_context.jet:7) — reading raw config\n"
        "  2. load_config (Examples/features/errors/error_context.jet:12) — loading config app.toml\n"
        "  3. run (Examples/features/errors/error_context.jet:16)\n";
    equal(jet_journey_render_styled(&journey,false,0),plain);
    equal(jet_journey_render_styled(&journey,false,80),
        "  failed here: read_raw (…/errors/error_context.jet:4)\n"
        " Trail [E3002] (3 hops via ?, origin first):\n"
        "  1. parse_config (…/errors/error_context.jet:7) — reading raw config\n"
        "  2. load_config (…/errors/error_context.jet:12) — loading config app.toml\n"
        "  3. run (…/errors/error_context.jet:16)\n");
    equal(jet_journey_render_styled(&journey,true,80),
        "\033[2;37m  failed here: read_raw (…/errors/error_context.jet:4)\033[0m\n"
        "\033[2;37m Trail [E3002] (3 hops via ?, origin first):\033[0m\n"
        "\033[2;37m  1. parse_config (…/errors/error_context.jet:7) — reading raw config\033[0m\n"
        "\033[2;37m  2. load_config (…/errors/error_context.jet:12) — loading config app.toml\033[0m\n"
        "\033[2;37m  3. run (…/errors/error_context.jet:16)\033[0m\n");
    equal(jet_journey_render_styled(&journey,false,40),
        "  failed here: read_raw (error_context.jet:4)\n"
        " Trail [E3002] (3 hops):\n"
        "  1. parse_config (error_context.jet:7)\n"
        "  2. load_config (error_context.jet:12)\n"
        "  3. run (error_context.jet:16)\n");
    // FORCE_COLOR pipe: ANSI changes no path budgets when width is absent.
    Text *colored=jet_journey_render_styled(&journey,true,0);
    char expected[2048]; size_t used=0; const char *start=plain;
    while (*start) {
        const char *end=strchr(start,'\n'); assert(end);
        int count=snprintf(expected+used,sizeof expected-used,"\033[2;37m%.*s\033[0m\n",(int)(end-start),start);
        assert(count>0 && (size_t)count<sizeof expected-used); used+=(size_t)count; start=end+1;
    }
    equal(colored,expected);
    file=borrowed("a.jet"); a=borrowed("g"); b=borrowed("f"); n1=borrowed("a very long note");
    origin=(Hop){&a,&file,1,1,&empty,1}; first=(Hop){&b,&file,2,1,&n1,1}; journey.hops.length=1;
    equal(jet_journey_render_styled(&journey,false,30),
        "  failed here: g (a.jet:1)\n Trail [E3002] (1 hop):\n  1. f (a.jet:2) — a very lon…\n");
    first.count=4; n1=empty;
    equal(jet_journey_render_styled(&journey,false,0),
        "  failed here: g (a.jet:1)\n Trail [E3002] (4 hops via ?, origin first):\n  1. f (a.jet:2) ×4\n");
    equal(jet_journey_render_styled(NULL,false,80),"");
}
static void journey_ownership(void) {
    Journey *value=jet_rt_native_journey_new();
    Text *file=text("a.jet"), *function=text("g"), *next=text("f"), *note=text("first"), *ignored=text("ignored");
    // A first call failure becomes origin; an explicit new origin resets it.
    jet_rt_native_journey_frame(value,file,1,7,function,note);
    assert(value->origin && value->origin->column==7 && !value->hops.length);
    jet_rt_native_journey_origin(value,file,1,8,function);
    assert(!value->origin->note->length && value->origin->column==8);
    jet_rt_native_journey_frame(value,file,2,9,next,note);
    jet_rt_native_journey_frame(value,file,2,99,next,ignored);
    assert(value->hops.length==1);
    Hop *held=((Hop **)value->hops.data)[0];
    assert(held->count==2 && held->column==9 && held->note->length==5 && !memcmp(held->note->data,"first",5));
    Journey *clone=jet_rt_native_journey_clone(value);
    assert(clone!=value && clone->origin!=value->origin && clone->hops.data!=value->hops.data);
    assert(((Hop **)clone->hops.data)[0]!=held && ((Hop **)clone->hops.data)[0]->count==2);
    jet_rt_native_journey_drop(value);
    equal(jet_journey_render_styled(clone,false,0),
        "  failed here: g (a.jet:1)\n Trail [E3002] (2 hops via ?, origin first):\n  1. f (a.jet:2) ×2 — first\n");
    Journey *carried=jet_rt_native_journey_new();
    // A carried origin replaces an old handled failure; all source children
    // move, not clone. The native caller drops neither carried nor its hops.
    jet_rt_native_journey_origin(carried,file,10,1,function);
    jet_rt_native_journey_frame(carried,file,11,1,next,note);
    Hop *moved_origin=carried->origin;
    jet_rt_native_journey_adopt(clone,carried);
    assert(clone->origin==moved_origin && clone->hops.length==1);
    carried=jet_rt_native_journey_new(); // No origin: incoming same-site collapse
    Hop *transferred=jet_rt_alloc(sizeof(*transferred),8);
    *transferred=(Hop){text("f"),text("a.jet"),11,99,text("discarded"),3};
    Hop **column=jet_rt_alloc(sizeof(*column),8); *column=transferred;
    carried->hops=(List){1,1,column};
    jet_rt_native_journey_adopt(clone,carried);
    assert(((Hop **)clone->hops.data)[0]->count==4);
    equal(jet_journey_render_styled(clone,false,0),
        "  failed here: g (a.jet:10)\n Trail [E3002] (4 hops via ?, origin first):\n  1. f (a.jet:11) ×4 — first\n");
    Text *message=text("error"); Err *error=jet_err_from_message(message); jet_rt_string_drop(message);
    Err *context=jet_rt_native_err_context_frame(error,clone,file,12,3,next,note);
    assert(!error->context.length && context->context.length==1);
    jet_rt_handle_drop_jet_err(error);
    jet_rt_native_journey_reset(clone);
    equal(jet_journey_render_styled(clone,false,0),"");
    Journey *slot=clone; assert(jet_rt_native_journey_take(&slot)==clone && !slot && !jet_rt_native_journey_take(&slot));
    assert(!jet_rt_journey_destructor(0,clone));
    equal(jet_render_err(context),"Error: error\n  context (a.jet:12): first");
    jet_rt_handle_drop_jet_err(context);
    jet_rt_string_drop(file); jet_rt_string_drop(function); jet_rt_string_drop(next);
    jet_rt_string_drop(note); jet_rt_string_drop(ignored);
    assert(jet_rt_program_allocator_fact(2)==0);
}
int main(void) {
    // Pack constructors initialize allocator/thread roots before measuring.
    W cap=jet_rt_program_allocator_fact(4), mode=jet_rt_program_allocator_fact(5);
    jet_rt_program_allocator_configure(1,0);
    journey_goldens();
    journey_ownership();
    Text *message=text("outer"), *code=text("E42"), *cause_message=text("inner");
    Err *cause=jet_err_from_message(cause_message);
    jet_rt_string_drop(cause_message);
    Err *value=jet_rt_record_new_jet_err(message,&code,cause); // cause TAKEN
    assert(value->cause==cause && value->message!=message && value->code!=code);
    jet_rt_string_drop(message); jet_rt_string_drop(code);
    Text *source=text("Source"), *target=text("Target");
    Err *converted=jet_err_apply_conversion(value,source,target);
    jet_rt_string_drop(source); jet_rt_string_drop(target); jet_rt_handle_drop_jet_err(value);
    Context *context=jet_rt_alloc(sizeof(*context),8);
    *context=(Context){text("note\n\t\"\\\177\302\200"),text("main.jet"),7};
    converted->context=(List){1,1,context};
    Details *details=jet_rt_alloc(sizeof(*details),8);
    Field *field=jet_rt_alloc(sizeof(*field),8);
    *field=(Field){text("n"),text("[1,{\"x\":true}]")};
    *details=(Details){text("Variant"),{1,1,field},1,10,20};
    converted->details=details;
    Err *clone=jet_rt_handle_clone_jet_err(converted);
    assert(clone!=converted && clone->cause!=converted->cause && clone->identity!=converted->identity);
    assert(clone->context.data!=converted->context.data && clone->conversions.data!=converted->conversions.data);
    assert(clone->details!=details && clone->details->fields.data!=details->fields.data);
    assert(jet_rt_handle_eq_jet_err(converted,clone));
    jet_rt_handle_drop_jet_err(converted);
    equal(jet_rt_record_get_jet_err_message(clone),"outer");
    Text *projection=NULL; assert(jet_rt_record_get_jet_err_code(clone,&projection)==1); equal(projection,"E42");
    Err *projected_cause=NULL; assert(jet_rt_record_get_jet_err_cause(clone,&projected_cause)==1);
    assert(projected_cause!=clone->cause); equal(jet_rt_record_get_jet_err_message(projected_cause),"inner");
    jet_rt_handle_drop_jet_err(projected_cause);
    equal(jet_rt_handle_display_jet_err(clone),"outer");
    equal(jet_rt_handle_show_jet_err(clone),"outer");
    equal(jet_rt_handle_debug_jet_err(clone),"outer");
    equal(jet_render_err(clone),"Error [E42]: outer (type: Source)\n  cause: inner\n  context (main.jet:7): note\n\t\"\\\177\302\200\n  conversion: Source -> Target");
    equal(jet_error_report_json(clone,NULL),"{\"schema\":\"jet.err/v1\",\"message\":\"outer\",\"code\":\"E42\",\"cause\":{\"schema\":\"jet.err/v1\",\"message\":\"inner\",\"code\":null,\"cause\":null},\"typed_identity\":\"Source\",\"context_frames\":[{\"text\":\"note\\n\\t\\\"\\\\\\u007f\\u0080\",\"file\":\"main.jet\",\"line\":7}],\"conversion_history\":[{\"source\":\"Source\",\"target\":\"Target\"}],\"details\":{\"variant\":\"Variant\",\"fields\":{\"n\":[1,{\"x\":true}]},\"source_span\":{\"start\":10,\"end\":20}}}");
    W getter_live=jet_rt_program_allocator_fact(2);
    for (unsigned i=0;i<1000;i++) {
        Text *m=jet_rt_record_get_jet_err_message(clone), *c=NULL;
        Err *cause_copy=NULL;
        assert(jet_rt_record_get_jet_err_code(clone,&c)==1);
        assert(jet_rt_record_get_jet_err_cause(clone,&cause_copy)==1);
        jet_rt_string_drop(m); jet_rt_string_drop(c);
        jet_rt_handle_drop_jet_err(cause_copy);
        assert(jet_rt_program_allocator_fact(2)==getter_live);
    }
    jet_rt_handle_drop_jet_err(clone);
    // Empty code differs from None; absent projections never initialize slots.
    message=text(""); code=text(""); value=jet_rt_record_new_jet_err(message,&code,NULL);
    jet_rt_string_drop(message); jet_rt_string_drop(code);
    equal(jet_render_err(value),"Error []: ");
    projected_cause=NULL; assert(!jet_rt_record_get_jet_err_cause(value,&projected_cause) && !projected_cause);
    jet_rt_handle_drop_jet_err(value);
    assert(jet_rt_program_allocator_fact(2)==0); // LIVE BYTES, not allocation history
    jet_rt_program_allocator_restore(mode,cap);
    puts("Err88: taken cause, deep clone/projection/metadata, root+wire bytes, ledger zero");
    return 0;
}
