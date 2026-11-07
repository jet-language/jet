.section .text.archive_export,"ax",@progbits
.globl jet_archive_fixture
.type jet_archive_fixture,@function
jet_archive_fixture:
    mov $5, %eax
    ret
.size jet_archive_fixture, .-jet_archive_fixture

// The platform object supplies a weak startup; the Jet runtime object overrides it.
.section .text.archive_start,"ax",@progbits
.weak _start
.type _start,@function
_start:
    call main
    mov %eax, %edi
    mov $60, %eax
    syscall
.size _start, .-_start
.section .note.GNU-stack,"",@progbits
