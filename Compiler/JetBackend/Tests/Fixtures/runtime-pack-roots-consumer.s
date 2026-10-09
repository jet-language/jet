.section .text.consumer,"ax",@progbits
.globl main
.type main,@function
main:
    sub $8, %rsp
    call jet_rt_fixture_unreferenced
    cmp $40, %eax
    jne .Lbad
    call jet_rt_fixture_weak
    cmp $2, %eax
    jne .Lbad
    call jet_rt_fixture_protected
    cmp $3, %eax
    jne .Lbad
    call jet_rt_fixture_export
    cmp $5, %eax
    jne .Lbad
    call jet_archive_fixture
    cmp $5, %eax
    jne .Lbad
    xor %eax, %eax
    add $8, %rsp
    ret
.Lbad:
    mov $1, %eax
    add $8, %rsp
    ret
.size main, .-main
.section .note.GNU-stack,"",@progbits
