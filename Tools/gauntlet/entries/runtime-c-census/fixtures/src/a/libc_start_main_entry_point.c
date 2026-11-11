extern void exit(int); extern int main(void); int __libc_start_main(void) { exit(main()); return 0; }
