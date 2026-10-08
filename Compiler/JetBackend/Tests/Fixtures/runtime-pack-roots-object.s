// ELF contract emitted by Object.jet: audited exports global, private functions local.
.section .text.pack_start,"ax",@progbits
.globl _start
.type _start,@function
_start:
    call main
    mov %eax, %edi
    mov $60, %eax
    syscall
.size _start, .-_start

.section .text.pack_export,"ax",@progbits
.globl jet_rt_fixture_export
.type jet_rt_fixture_export,@function
jet_rt_fixture_export:
    sub $8, %rsp
    call pack_private_helper
    call jet_pack_hidden_helper
    call _Rpack_private_helper
    call jet_archive_fixture
    add $8, %rsp
    ret
.size jet_rt_fixture_export, .-jet_rt_fixture_export

// Nothing in either input calls this public export: rooting must keep its section.
.section .text.pack_unreferenced,"ax",@progbits
.globl jet_rt_fixture_unreferenced
.type jet_rt_fixture_unreferenced,@function
jet_rt_fixture_unreferenced:
    mov $40, %eax
    ret
.size jet_rt_fixture_unreferenced, .-jet_rt_fixture_unreferenced

.section .text.pack_weak,"ax",@progbits
.weak jet_rt_fixture_weak
.type jet_rt_fixture_weak,@function
jet_rt_fixture_weak:
    mov $2, %eax
    ret
.size jet_rt_fixture_weak, .-jet_rt_fixture_weak

.section .text.pack_protected,"ax",@progbits
.globl jet_rt_fixture_protected
.protected jet_rt_fixture_protected
.type jet_rt_fixture_protected,@function
jet_rt_fixture_protected:
    mov $3, %eax
    ret
.size jet_rt_fixture_protected, .-jet_rt_fixture_protected

.section .text.pack_private,"ax",@progbits
.type pack_private_helper,@function
pack_private_helper:
    ret
.size pack_private_helper, .-pack_private_helper

.section .text.pack_hidden,"ax",@progbits
.globl jet_pack_hidden_helper
.hidden jet_pack_hidden_helper
.type jet_pack_hidden_helper,@function
jet_pack_hidden_helper:
    ret
.size jet_pack_hidden_helper, .-jet_pack_hidden_helper

.section .text.pack_mangled,"ax",@progbits
.globl _Rpack_private_helper
.type _Rpack_private_helper,@function
_Rpack_private_helper:
    ret
.size _Rpack_private_helper, .-_Rpack_private_helper

.section .text.pack_dead_hidden,"ax",@progbits
.globl jet_pack_dead_hidden
.hidden jet_pack_dead_hidden
.type jet_pack_dead_hidden,@function
jet_pack_dead_hidden:
    ret
.size jet_pack_dead_hidden, .-jet_pack_dead_hidden

.section .text.pack_dead_private,"ax",@progbits
.type pack_dead_private,@function
pack_dead_private:
    ret
.size pack_dead_private, .-pack_dead_private
.section .note.GNU-stack,"",@progbits
