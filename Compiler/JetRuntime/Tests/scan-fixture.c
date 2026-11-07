/* Raw scan ABI and Float-bit witness host: run-scan-goldens.mjs lowers the
 * runtime sources plus ScanTests.jet and the generated float corpus to one
 * fixture object. A failed case exits 1 (scan_test_require); success is silent.
 */
#include <stdint.h>

extern void jet_rt_host_arguments(int64_t, char **, char **);
extern void jet_rt_thread_attach(void);
extern void jet_rt_thread_detach(void);
extern void jet_rt_scan_goldens_fixture(void);

int main(int argc, char **argv, char **envp) {
    jet_rt_host_arguments(argc, argv, envp);
    jet_rt_thread_attach();
    jet_rt_scan_goldens_fixture();
    jet_rt_thread_detach();
    return 0;
}
