#include <pthread.h>
#include <stdint.h>
#include <unistd.h>
extern int64_t jet_perl_ops_open(void);
extern const char* jet_perl_ops_invoke_sleep(int64_t,const char*,int64_t);
extern void jet_perl_ops_cancel(int64_t);
extern void jet_perl_ops_close(int64_t);
extern int64_t jet_perl_ops_take_error(void);
static int64_t handle;static int64_t code;
static void* call(void*unused){(void)unused;jet_perl_ops_invoke_sleep(handle,"null",60000);code=jet_perl_ops_take_error();return 0;}
int main(void){handle=jet_perl_ops_open();if(!handle)return 1;pthread_t thread;if(pthread_create(&thread,0,call,0))return 2;usleep(100000);jet_perl_ops_cancel(handle);pthread_join(thread,0);if(code!=3)return 3;int64_t fresh=jet_perl_ops_open();if(!fresh)return 4;jet_perl_ops_close(fresh);return 0;}
