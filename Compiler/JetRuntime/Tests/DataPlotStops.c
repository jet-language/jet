/* Real Jet providers only. Separate process modes must exit 70 and match
 * DataPlotStops-{ctor-range,getter-variant}.stderr byte-for-byte.
 * Ctor range_stop: CAbiRoutes.rs:11-13,9766; wrong variant:10137-10139. */
#include <stdint.h>
#include <string.h>
extern void jet_rt_alloc_init(void);
extern int64_t jet_rt_record_new_jet_data_plot_axis(int64_t, int64_t, uint8_t, uint8_t, int64_t);
extern int64_t jet_rt_record_new_jet_data_plot_value_null(void);
extern int64_t jet_rt_record_get_jet_data_plot_value_integer(int64_t);
int main(int argc, char **argv) {
    if (argc != 2) return 64;
    jet_rt_alloc_init();
    if (strcmp(argv[1], "ctor-range") == 0) {
        /* No text or Int conversion/allocation may occur before ordinal stop. */
        (void)jet_rt_record_new_jet_data_plot_axis(6, 0, 0, 0, 0);
    } else if (strcmp(argv[1], "getter-variant") == 0) {
        int64_t null_value = jet_rt_record_new_jet_data_plot_value_null();
        (void)jet_rt_record_get_jet_data_plot_value_integer(null_value);
    } else return 64;
    return 99; /* Reaching this is a real provider failure, not a fallback. */
}
