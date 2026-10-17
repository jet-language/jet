/* Link the actual composed Jet runtime with --wrap=jet_rt_free.
 * message_parts has 12 arguments: bcc/html/attachment/envelope/wire exercise
 * stack argument slots. This client supplies borrowed native carriers. */
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { unsigned char *data; int64_t len,cap; } Text;
typedef struct { Text *display,*mailbox; } Address;
typedef struct { int64_t len,cap; Address *data; } Addresses;
typedef struct { int64_t len,cap; unsigned char *data; } Bytes;
typedef struct { Text *filename,*mime; Bytes bytes; } Attachment;
typedef struct { int64_t len,cap; Attachment *data; } Attachments;
typedef struct { Address from; Addresses recipients; } Envelope;
typedef struct { Address from; Addresses to,bcc; Text *subject,*text,*html; Attachments attachments; Envelope envelope; int64_t wire_upper; } Message;
_Static_assert(sizeof(Address)==16 && sizeof(Envelope)==40,"inline owners");
_Static_assert(sizeof(Message)==160 && offsetof(Message,to)==16 && offsetof(Message,bcc)==40 && offsetof(Message,subject)==64 && offsetof(Message,attachments)==88 && offsetof(Message,envelope)==112 && offsetof(Message,wire_upper)==152,"Message160");
extern void jet_rt_alloc_init(void);
extern void *jet_rt_alloc(int64_t,int64_t);
extern void __real_jet_rt_free(void *,int64_t,int64_t);
extern Text *jet_rt_string_clone(Text *);
extern void jet_rt_string_drop(Text *);
extern int64_t jet_int_owned_from_i64(int64_t);
extern int64_t jet_int_owned_to_i64(int64_t);
extern void jet_rt_int_drop(int64_t);
extern Message *message_parts(Address *,Address **,int64_t,Address **,int64_t,Text *,Text *,Text *,Attachment **,int64_t,Envelope *,int64_t);
extern Message *jet_rt_handle_clone_message(Message *);
extern void jet_rt_handle_drop_message(Message *);
extern bool jet_rt_handle_eq_message(Message *,Message *);
extern Address *jet_rt_handle_field_message_from(Message *);
extern Envelope *jet_rt_handle_field_message_envelope(Message *);
extern void jet_rt_handle_field_message_subject(Message *,Text *);
extern void jet_rt_handle_field_message_text(Message *,Text *);
extern void jet_rt_handle_field_message_html(Message *,Text *);
struct Watched { void *ptr; int64_t size,align; unsigned freed; };
static struct Watched watched[8192];
static size_t count;
static void watch(void *ptr,int64_t size,int64_t align) {
    assert(ptr && count<sizeof(watched)/sizeof(watched[0]));
    for(size_t i=0;i<count;++i) assert(watched[i].ptr!=ptr || watched[i].freed);
    watched[count++]=(struct Watched){ptr,size,align,0};
}
void __wrap_jet_rt_free(void *ptr,int64_t size,int64_t align) {
    for(size_t i=count;i>0;--i) if(watched[i-1].ptr==ptr && !watched[i-1].freed) {
        assert(size==watched[i-1].size && align==watched[i-1].align);
        watched[i-1].freed=1; break;
    }
    __real_jet_rt_free(ptr,size,align);
}
static void text_watch(Text *t) { watch((char *)(t)-8,32,8); if(t->cap) watch(t->data,t->cap,1); }
static void address_watch(Address *a) { if(a->display) text_watch(a->display); text_watch(a->mailbox); }
static void addresses_watch(Addresses *a) {
    if(a->cap) watch(a->data,a->cap*16,8);
    for(int64_t i=0;i<a->len;++i) address_watch(a->data+i);
}
static void message_watch(Message *m) {
    watch(m,160,8); address_watch(&m->from); addresses_watch(&m->to); addresses_watch(&m->bcc);
    text_watch(m->subject); text_watch(m->text); text_watch(m->html);
    if(m->attachments.cap) watch(m->attachments.data,m->attachments.cap*40,8);
    for(int64_t i=0;i<m->attachments.len;++i) {
        Attachment *a=m->attachments.data+i; text_watch(a->filename); text_watch(a->mime);
        if(a->bytes.cap) watch(a->bytes.data,a->bytes.cap,8);
    }
    address_watch(&m->envelope.from); addresses_watch(&m->envelope.recipients);
}
static void text_equal(Text *a,Text *b) { assert(a->len==b->len && !memcmp(a->data,b->data,(size_t)a->len)); }
static void independent(Text *a,Text *b) { assert(a!=b); if(a->len) assert(a->data!=b->data); text_equal(a,b); }
static void address_independent(Address *a,Address *b) {
    assert((a->display==NULL)==(b->display==NULL));
    if(a->display) independent(a->display,b->display);
    independent(a->mailbox,b->mailbox);
}
// Grow a real owner's container allocation, preserving initialized fields.
// Only the old buffer is released; child owners move into the replacement.
static void spare_addresses(Addresses *a) {
    Address *next=jet_rt_alloc((a->len+3)*16,8);
    memcpy(next,a->data,(size_t)a->len*16);
    __wrap_jet_rt_free(a->data,a->cap*16,8);
    a->data=next; a->cap=a->len+3; watch(next,a->cap*16,8);
}
#define MUTATE_BYTE(pointer) do { (pointer)[0]^=1; assert(!jet_rt_handle_eq_message(a,b)); (pointer)[0]^=1; } while(0)
int main(void) {
    jet_rt_alloc_init(); jet_rt_handle_drop_message(NULL);
    unsigned char binary[]={'x',0,0xc3,0xa9},payload[]={0,255,17,0};
    Text input={binary,4,4},empty={(unsigned char *)"",0,0};
    const int64_t wires[]={INT64_MIN,-1,0,7096,INT64_MAX};
    for(size_t w=0;w<5;++w) for(unsigned mode=0;mode<3;++mode) {
        Text *owned=jet_rt_string_clone(&input),*display=jet_rt_string_clone(mode==2 ? &empty : &input);
        text_watch(owned); text_watch(display);
        Address from={mode ? display : NULL,owned},recipient={NULL,owned};
        Address *to[]={&from,&recipient},*bcc[]={&recipient};
        Attachment attachment={owned,owned,{4,4,payload}},*attachments[]={&attachment};
        Address env_members[]={recipient,from}; Envelope envelope={from,{2,2,env_members}};
        int64_t exact=jet_int_owned_from_i64(wires[w]);
        Message *a=message_parts(&from,to,2,bcc,1,owned,owned,owned,attachments,1,&envelope,exact);
        message_watch(a); assert(jet_int_owned_to_i64(exact)==wires[w]); jet_rt_int_drop(exact);
        assert(a->wire_upper==wires[w] && a->to.len==2 && a->bcc.len==1 && a->attachments.len==1);
        address_independent(&from,&a->from); address_independent(to[0],a->to.data); address_independent(bcc[0],a->bcc.data);
        address_independent(&envelope.from,&a->envelope.from); address_independent(env_members,a->envelope.recipients.data);
        independent(owned,a->subject); independent(owned,a->text); independent(owned,a->html);
        independent(owned,a->attachments.data[0].filename); independent(owned,a->attachments.data[0].mime);
        assert(a->attachments.data[0].bytes.data!=payload && a->attachments.data[0].bytes.len==4 && !memcmp(a->attachments.data[0].bytes.data,payload,4));
        assert(a->to.data!=(Address *)to && a->bcc.data!=(Address *)bcc && a->attachments.data!=(Attachment *)attachments);
        jet_rt_string_drop(owned); jet_rt_string_drop(display);
        Message *b=jet_rt_handle_clone_message(a); message_watch(b); assert(a!=b && jet_rt_handle_eq_message(a,b));
        address_independent(&a->from,&b->from); address_independent(a->to.data,b->to.data); address_independent(a->bcc.data,b->bcc.data);
        address_independent(&a->envelope.from,&b->envelope.from); address_independent(a->envelope.recipients.data,b->envelope.recipients.data);
        independent(a->subject,b->subject); independent(a->text,b->text); independent(a->html,b->html);
        independent(a->attachments.data[0].filename,b->attachments.data[0].filename); independent(a->attachments.data[0].mime,b->attachments.data[0].mime);
        assert(a->attachments.data[0].bytes.data!=b->attachments.data[0].bytes.data);
        MUTATE_BYTE(b->from.mailbox->data);
        spare_addresses(&a->to); spare_addresses(&a->bcc); spare_addresses(&a->envelope.recipients);
        if(b->from.display && b->from.display->len) MUTATE_BYTE(b->from.display->data);
        Text *saved=b->from.display; b->from.display=saved ? NULL : b->subject;
        assert(!jet_rt_handle_eq_message(a,b)); b->from.display=saved;
        MUTATE_BYTE(b->to.data[0].mailbox->data); MUTATE_BYTE(b->bcc.data[0].mailbox->data);
        MUTATE_BYTE(b->subject->data); MUTATE_BYTE(b->text->data); MUTATE_BYTE(b->html->data);
        MUTATE_BYTE(b->attachments.data[0].filename->data); MUTATE_BYTE(b->attachments.data[0].mime->data); MUTATE_BYTE(b->attachments.data[0].bytes.data);
        MUTATE_BYTE(b->envelope.from.mailbox->data); MUTATE_BYTE(b->envelope.recipients.data[0].mailbox->data);
        b->wire_upper^=1; assert(!jet_rt_handle_eq_message(a,b)); b->wire_upper^=1;
        b->to.len=1; assert(!jet_rt_handle_eq_message(a,b)); b->to.len=2;
        b->bcc.len=0; assert(!jet_rt_handle_eq_message(a,b)); b->bcc.len=1;
        b->attachments.len=0; assert(!jet_rt_handle_eq_message(a,b)); b->attachments.len=1;
        b->envelope.recipients.len=1; assert(!jet_rt_handle_eq_message(a,b)); b->envelope.recipients.len=2;
        b->attachments.data[0].bytes.len=3; assert(!jet_rt_handle_eq_message(a,b)); b->attachments.data[0].bytes.len=4;
        b->to.cap+=3; b->bcc.cap+=2; b->attachments.cap+=1; b->envelope.recipients.cap+=4; b->attachments.data[0].bytes.cap+=7;
        assert(jet_rt_handle_eq_message(a,b)); b->to.cap-=3; b->bcc.cap-=2; b->attachments.cap-=1; b->envelope.recipients.cap-=4; b->attachments.data[0].bytes.cap-=7;
        assert(jet_rt_handle_field_message_from(a)==&a->from && jet_rt_handle_field_message_envelope(a)==&a->envelope);
        Text subject,text,html; jet_rt_handle_field_message_subject(a,&subject); jet_rt_handle_field_message_text(a,&text); jet_rt_handle_field_message_html(a,&html);
        assert(subject.data==a->subject->data && subject.len==4 && subject.cap==a->subject->cap);
        assert(text.data==a->text->data && html.data==a->html->data);
        Text *survivor=jet_rt_string_clone(&subject); text_watch(survivor);
        jet_rt_handle_drop_message(a); text_equal(survivor,&input); text_equal(b->subject,&input);
        jet_rt_string_drop(survivor); jet_rt_handle_drop_message(b);
    }
    Address from={NULL,&empty}; Envelope env={from,{0,0,NULL}};
    int64_t zero=jet_int_owned_from_i64(0);
    Message *a=message_parts(&from,NULL,7,NULL,-1,&empty,&empty,&empty,NULL,7,&env,zero);
    message_watch(a); jet_rt_int_drop(zero);
    assert(!a->to.len && !a->bcc.len && !a->attachments.len);
    Message *b=jet_rt_handle_clone_message(a); message_watch(b); assert(jet_rt_handle_eq_message(a,b));
    jet_rt_handle_drop_message(a); jet_rt_handle_drop_message(b);
    for(size_t i=0;i<count;++i) assert(watched[i].freed==1);
    puts("message: 12-arg borrowed parts, inline Address/Envelope and bcc/Attachment owners, packed bytes, deep clone/all-field equality, exact wire edges, borrowed projections, binary/empty data, exact frees, null drop OK");
    return 0;
}
