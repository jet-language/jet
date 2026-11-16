#include <stdint.h>
extern int64_t jet_pascal_inventory_counter_new(int64_t);
extern void jet_pascal_inventory_counter_close(int64_t);
extern int64_t jet_pascal_inventory_take_error(void);
extern int64_t jet_pascal_inventory_destroyed_count(void);
int main(void){int64_t first=jet_pascal_inventory_counter_new(1);if(!first)return 1;jet_pascal_inventory_counter_close(first);if(jet_pascal_inventory_destroyed_count()!=1)return 2;int64_t second=jet_pascal_inventory_counter_new(2);if(!second||second==first)return 3;jet_pascal_inventory_counter_close(first);if(jet_pascal_inventory_take_error()!=1)return 4;if(jet_pascal_inventory_destroyed_count()!=1)return 5;jet_pascal_inventory_counter_close(second);if(jet_pascal_inventory_destroyed_count()!=2)return 6;return 0;}
