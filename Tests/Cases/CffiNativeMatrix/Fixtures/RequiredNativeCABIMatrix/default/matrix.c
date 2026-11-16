
#include <stddef.h>
#include <stdint.h>

typedef struct { int64_t x; int64_t y; } Coord;
typedef int32_t Status;
typedef uint8_t PacketTag;
typedef union { int64_t ping; struct { int64_t x; int64_t y; } data; } PacketPayload;
typedef struct { PacketTag tag; PacketPayload payload; } Packet;

int64_t coord_sum(Coord p) { return p.x + p.y; }
int32_t status_value(Status s) { return s; }
int32_t packet_value(Packet p) { return (int32_t)p.tag * 100 + (int32_t)p.payload.ping; }
int32_t packet_size(void) { return (int32_t)sizeof(Packet); }
int32_t packet_align(void) { struct A { char c; Packet p; }; return (int32_t)offsetof(struct A, p); }
int32_t packet_payload_offset(void) { return (int32_t)offsetof(Packet, payload); }

typedef int32_t (*callback_t)(int32_t);
int32_t callback_twice(callback_t cb, int32_t x) { return cb(cb(x)); }

#include <pthread.h>
typedef struct { callback_t cb; int32_t x; int32_t out; } Job;
static void *run_job(void *raw) { Job *j = (Job *)raw; j->out = j->cb(j->x); return 0; }
int32_t callback_parallel(callback_t cb) {
    pthread_t threads[4]; Job jobs[4]; int32_t sum = 0;
    for (int i=0;i<4;i++) { jobs[i]=(Job){cb,i,0}; if(pthread_create(&threads[i],0,run_job,&jobs[i])) return -1000; }
    for (int i=0;i<4;i++) { pthread_join(threads[i],0); sum += jobs[i].out; }
    return sum;
}

typedef struct { uint64_t id; uint32_t flags; } Record;
int32_t load_record(uint64_t id, Record *out) {
    if (id == 7) { out->id=70; out->flags=3; return 0; }
    unsigned char *p=(unsigned char *)out; for(size_t i=0;i<sizeof(*out);i++) p[i]=0xA5;
    return 9;
}

int32_t abi_default(int32_t a, int32_t b) { return a + b; }
int32_t abi_explicit(int32_t a, int32_t b) { return a + b; }
