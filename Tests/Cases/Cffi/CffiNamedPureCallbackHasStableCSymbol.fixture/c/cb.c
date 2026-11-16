#include <stdint.h>
#include <pthread.h>
typedef int32_t (*cb_t)(int32_t);
int32_t call_twice(cb_t cb,int32_t x){ return cb(cb(x)); }
typedef struct { cb_t cb; int32_t x; int32_t out; } Job;
static void* run_job(void* p){ Job* j=(Job*)p; j->out=j->cb(j->x); return 0; }
int32_t call_parallel(cb_t cb){ pthread_t t[4]; Job j[4]; for(int i=0;i<4;i++){j[i]=(Job){cb,i,0}; pthread_create(&t[i],0,run_job,&j[i]);} int32_t s=0; for(int i=0;i<4;i++){pthread_join(t[i],0);s+=j[i].out;} return s; }
