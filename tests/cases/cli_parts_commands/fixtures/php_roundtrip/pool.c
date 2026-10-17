#include <pthread.h>
#include <stdint.h>
#include <time.h>
#include <unistd.h>
extern int64_t jet_php_ops_open(void);
extern const char* jet_php_ops_invoke_pooled_sleep(int64_t,const char*,int64_t);
extern const char* jet_php_ops_invoke_sleep_call(int64_t,const char*,int64_t);
extern const char* jet_php_ops_invoke_transform(int64_t,const char*,int64_t);
extern void jet_php_ops_cancel(int64_t);
extern void jet_php_ops_close(int64_t);
extern int64_t jet_php_ops_take_error(void);
static int64_t pool;static int64_t codes[4];
static void* parallel_call(void*arg){intptr_t i=(intptr_t)arg;jet_php_ops_invoke_pooled_sleep(pool,"null",5000);codes[i]=jet_php_ops_take_error();return 0;}
static void* cancel_call(void*unused){(void)unused;jet_php_ops_invoke_sleep_call(pool,"null",60000);codes[0]=jet_php_ops_take_error();return 0;}
static int64_t millis(void){struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);return (int64_t)t.tv_sec*1000+t.tv_nsec/1000000;}
int main(void){const char*valid="{\"nested\":{},\"list\":[],\"scalar\":1,\"nothing\":null}";pool=jet_php_ops_open();if(!pool)return 1;pthread_t threads[4];int64_t start=millis();for(intptr_t i=0;i<4;i++)if(pthread_create(&threads[i],0,parallel_call,(void*)i))return 2;for(int i=0;i<4;i++)pthread_join(threads[i],0);if(millis()-start>2500)return 3;for(int i=0;i<4;i++)if(codes[i])return 4;jet_php_ops_invoke_sleep_call(pool,"null",100);if(jet_php_ops_take_error()!=2)return 5;jet_php_ops_invoke_transform(pool,valid,5000);int64_t recovery=jet_php_ops_take_error();if(recovery)return 20+(int)recovery;pthread_t cancelled;if(pthread_create(&cancelled,0,cancel_call,0))return 7;usleep(100000);jet_php_ops_cancel(pool);pthread_join(cancelled,0);if(codes[0]!=3)return 8;for(int i=0;i<4;i++){jet_php_ops_invoke_transform(pool,valid,5000);int64_t code=jet_php_ops_take_error();if(code)return 30+(int)code;}jet_php_ops_close(pool);int64_t pools[8];for(int i=0;i<8;i++)if(!(pools[i]=jet_php_ops_open()))return 40+i;if(jet_php_ops_open()!=0||jet_php_ops_take_error()!=1)return 49;for(int i=0;i<8;i++)jet_php_ops_close(pools[i]);return 0;}
