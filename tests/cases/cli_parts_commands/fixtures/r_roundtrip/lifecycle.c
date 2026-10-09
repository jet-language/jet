#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
extern int64_t jet_r_ops_open(void);
extern const char* jet_r_ops_invoke_sleep_call(int64_t,const char*,int64_t);
extern const char* jet_r_ops_invoke_sleep_call_plot(int64_t,const char*,int64_t);
extern const char* jet_r_ops_invoke_plot_scores_plot(int64_t,const char*,int64_t);
extern const char* jet_r_ops_invoke_hostile_plot_plot(int64_t,const char*,int64_t);
extern const char* jet_r_ops_invoke_transform(int64_t,const char*,int64_t);
extern void jet_r_ops_cancel(int64_t);
extern void jet_r_ops_close(int64_t);
extern int64_t jet_r_ops_take_error(void);
static int64_t handle;static int64_t code;
static void* call(void*unused){(void)unused;jet_r_ops_invoke_sleep_call_plot(handle,"1",60000);code=jet_r_ops_take_error();return 0;}
static int hostile(int64_t h,const char*kind){char input[64];snprintf(input,sizeof(input),"{\"kind\":\"%s\"}",kind);const char*response=jet_r_ops_invoke_hostile_plot_plot(h,input,5000);if(jet_r_ops_take_error()!=0||!response||!strstr(response,"\"ok\":false")||strstr(response,"secret"))return 1;return 0;}
int main(void){handle=jet_r_ops_open();if(!handle)return 1;const char*svg=jet_r_ops_invoke_plot_scores_plot(handle,"{\"values\":[2,5,3]}",5000);if(jet_r_ops_take_error()!=0||!svg||!strstr(svg,"\"ok\":true")||!strstr(svg,"<svg height=\\\"")||strstr(svg,"<?xml")||strstr(svg,"<script"))return 2;const char*kinds[]={"script","event","foreign","external","css","doctype","malformed","oversize"};for(int i=0;i<8;i++)if(hostile(handle,kinds[i]))return 10+i;const char*recovered=jet_r_ops_invoke_transform(handle,"{\"nested\":{},\"vector\":[1,2],\"scalar\":1,\"nothing\":null}",5000);if(jet_r_ops_take_error()!=0||!recovered||!strstr(recovered,"\"ok\":true"))return 20;jet_r_ops_invoke_sleep_call_plot(handle,"1",100);if(jet_r_ops_take_error()!=2)return 21;int64_t timed=jet_r_ops_open();if(!timed)return 22;svg=jet_r_ops_invoke_plot_scores_plot(timed,"{\"values\":[2,5,3]}",5000);if(jet_r_ops_take_error()!=0||!svg||!strstr(svg,"\"ok\":true"))return 23;jet_r_ops_close(timed);handle=jet_r_ops_open();if(!handle)return 24;pthread_t thread;if(pthread_create(&thread,0,call,0))return 25;usleep(100000);jet_r_ops_cancel(handle);pthread_join(thread,0);if(code!=3)return 26;int64_t fresh=jet_r_ops_open();if(!fresh)return 27;svg=jet_r_ops_invoke_plot_scores_plot(fresh,"{\"values\":[2,5,3]}",5000);if(jet_r_ops_take_error()!=0||!svg||!strstr(svg,"\"ok\":true"))return 28;jet_r_ops_close(fresh);int64_t sessions[32];for(int i=0;i<32;i++)if(!(sessions[i]=jet_r_ops_open()))return 40+i;if(jet_r_ops_open()!=0||jet_r_ops_take_error()!=1)return 72;for(int i=0;i<32;i++)jet_r_ops_close(sessions[i]);return 0;}
