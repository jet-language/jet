#include <stdint.h>
extern int64_t jet_pascal_inventory_counter_new(int64_t);
extern void jet_pascal_inventory_counter_close(int64_t);
extern int64_t jet_pascal_inventory_take_error(void);
int main(void){int64_t handles[64];for(int i=0;i<64;i++){handles[i]=jet_pascal_inventory_counter_new(i);if(!handles[i])return 1;}if(jet_pascal_inventory_counter_new(65)!=0||jet_pascal_inventory_take_error()!=2)return 2;for(int i=0;i<64;i++){jet_pascal_inventory_counter_close(handles[i]);if(jet_pascal_inventory_take_error()!=0)return 3;}return 0;}
