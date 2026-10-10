.section .text.missing_export,"ax",@progbits
.globl jet_rt_fixture_missing
.type jet_rt_fixture_missing,@function
jet_rt_fixture_missing:
    sub $8, %rsp
    call jet_ws_connect
    add $8, %rsp
    ret
.size jet_rt_fixture_missing, .-jet_rt_fixture_missing
.section .note.GNU-stack,"",@progbits
