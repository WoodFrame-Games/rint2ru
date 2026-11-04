bits 64
default rel

%define SYS_WRITE 1
%define SYS_EXIT 60
%define SYS_CLK 228
%define LO 100000000000000000
%define SPAN 8900000000000000000
%define MAGIC10 0xCCCCCCCCCCCCCCCD

section .data
msg_head db "========== РЕЗУЛЬТАТ (ASM) ==========", 10
.len equ $ - msg_head
msg_found db "Найденное число: "
.len equ $ - msg_found
msg_tries db 10, "Всего генераций: "
.len equ $ - msg_tries
msg_time db 10, "Время поиска: "
.len equ $ - msg_time
msg_secsp db " сек "
.len equ $ - msg_secsp
msg_ms db " мс", 10
.len equ $ - msg_ms
msg_speed db "Скорость: "
.len equ $ - msg_speed
msg_speed2 db " генераций/сек", 10
.len equ $ - msg_speed2
msg_threads db "Потоков: 1", 10
.len equ $ - msg_threads
msg_nl db 10
.len equ $ - msg_nl

section .bss
ts0 resq 2
ts1 resq 2
outbuf resb 40
seed resq 1

section .text
global _start

%ifdef TESTU64
_start:
    mov rdi, 1234567890123456789
    call u64str
    call print
    mov rsi, msg_nl
    mov rdx, msg_nl.len
    call print
    xor edi, edi
    mov eax, SYS_EXIT
    syscall
%else
_start:
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15

    mov edi, 1
    lea rsi, [ts0]
    mov eax, SYS_CLK
    syscall

    mov rax, [ts0]
    xor rax, [ts0 + 8]
    jnz .seed_ok
    mov eax, 0x9E3779B9
.seed_ok:
    mov [seed], rax

    xor ebp, ebp

.loop:
    mov rax, [seed]
    mov rcx, rax
    shr rcx, 12
    xor rax, rcx
    mov rcx, rax
    shl rcx, 25
    xor rax, rcx
    mov rcx, rax
    shr rcx, 27
    xor rax, rcx
    mov [seed], rax
    mov r11, 0x2545F4914F6CDD1D
    mul r11

    xor edx, edx
    mov r10, SPAN
    div r10
    mov r11, LO
    lea rbx, [rdx + r11]
    inc rbp

    mov rdi, rbx
    call u64str
    mov r14, rsi
    mov r13, rdx
    sub r13, 10
    xor r15, r15
    mov r8, "17181036"
.scan:
    cmp r15, r13
    jg .nofound
    cmp qword [r14 + r15], r8
    jne .nextpos
    cmp word [r14 + r15 + 8], "20"
    je .found
.nextpos:
    inc r15
    jmp .scan
.nofound:
    jmp .loop

.found:
    mov edi, 1
    lea rsi, [ts1]
    mov eax, SYS_CLK
    syscall

    mov rsi, msg_head
    mov rdx, msg_head.len
    call print

    mov rsi, msg_found
    mov rdx, msg_found.len
    call print
    mov rdi, rbx
    call u64str
    call print

    mov rsi, msg_tries
    mov rdx, msg_tries.len
    call print
    mov rdi, rbp
    call u64str
    call print

    mov rax, [ts1]
    sub rax, [ts0]
    mov r10, 1000000000
    mul r10
    mov r11, rax
    mov rax, [ts1 + 8]
    sub rax, [ts0 + 8]
    add rax, r11
    mov rbx, rax

    mov rsi, msg_time
    mov rdx, msg_time.len
    call print
    mov rax, rbx
    xor edx, edx
    mov r10, 1000000000
    div r10
    push rdx
    mov rdi, rax
    call u64str
    call print
    mov rsi, msg_secsp
    mov rdx, msg_secsp.len
    call print
    pop rax
    xor edx, edx
    mov r10, 1000000
    div r10
    mov rdi, rax
    call u64str
    call print
    mov rsi, msg_ms
    mov rdx, msg_ms.len
    call print

    mov rsi, msg_speed
    mov rdx, msg_speed.len
    call print
    mov rax, rbp
    mov r10, 1000000000
    mul r10
    div rbx
    mov rdi, rax
    call u64str
    call print
    mov rsi, msg_speed2
    mov rdx, msg_speed2.len
    call print

    mov rsi, msg_threads
    mov rdx, msg_threads.len
    call print

    xor edi, edi
    mov eax, SYS_EXIT
    syscall
%endif

print:
    mov rax, SYS_WRITE
    mov rdi, 1
    syscall
    ret

u64str:
    mov rax, rdi
    mov r9, MAGIC10
    mov r8, outbuf + 30
    xor ecx, ecx
.d:
    mov r11, rax
    mul r9
    mov r10, rdx
    shr r10, 3
    lea rdx, [r10 + r10*4]
    add rdx, rdx
    mov rax, r11
    sub rax, rdx
    add al, '0'
    mov [r8], al
    dec r8
    inc rcx
    mov rax, r10
    test rax, rax
    jnz .d
    inc r8
    mov rsi, r8
    mov rdx, rcx
    ret

section .note.GNU-stack noalloc noexec nowrite progbits
