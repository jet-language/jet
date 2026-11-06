#include <stdint.h>
#include <stddef.h>
typedef enum { STATUS_OK=0, STATUS_LOST=7 } Status;
typedef enum __attribute__((packed)) { PACKET_PING=3, PACKET_DATA=7 } PacketTag;
typedef union { long long ping; struct { long long x; long long y; } data; } PacketPayload;
typedef struct { PacketTag tag; PacketPayload payload; } Packet;
int32_t repr_status(Status s){return (int32_t)s;} int32_t repr_packet(Packet p){return (int32_t)p.tag*100+p.payload.ping;}
int32_t repr_packet_size(void){return sizeof(Packet);} int32_t repr_packet_align(void){return _Alignof(Packet);}
int32_t repr_packet_payload_offset(void){return offsetof(Packet,payload);}
