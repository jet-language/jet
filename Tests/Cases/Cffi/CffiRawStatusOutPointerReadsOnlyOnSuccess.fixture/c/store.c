#include <stdint.h>
#include <string.h>
typedef struct { uint64_t id; uint32_t flags; } Record;
int32_t store_load(uint64_t id, Record* out){ if(id==7){out->id=70;out->flags=3;return 0;} memset(out,0xA5,sizeof(*out)); return 9; }
