bits 64
default rel

%define SYS_WRITE   1
%define SYS_EXIT    60
%define SYS_EXITGRP 231
%define SYS_CLK     228
%define SYS_CLONE   56
%define SYS_YIELD   24
%define SYS_NANOSLEEP 35

%define CHUNKS 128
%define LEN    60
%define NEWD   7680
%define S        7712
%define PMAX     7688
%define RUNTIME  20
%define CLONE_FLAGS 0x00050F00

section .data
msg_head db "=== ASM AVX-512: поиск привет+друг, 2 потока, 20 сек ===", 10
.len equ $ - msg_head
msg_dig  db "цифр пройдено: "
.len equ $ - msg_dig
msg_time db 10, "время: "
.len equ $ - msg_time
msg_sec  db " сек", 10
.len equ $ - msg_sec
msg_rate db "скорость: "
.len equ $ - msg_rate
msg_mn   db " млн цифр/сек", 10
.len equ $ - msg_mn
msg_p1   db "вхождений привет: "
.len equ $ - msg_p1
msg_pairs db 10, "пар с зазором <= 5: "
.len equ $ - msg_pairs
msg_nl   db 10
.len equ $ - msg_nl

align 64
ones_c   times 64 db '1'
align 64
m100_c   times 16 dd 0x51EB851F
align 64
c100_c   times 16 dd 100
align 64
c205_c   times 16 dd 205
align 64
c10_c    times 16 dd 10
align 64
char0_c  times 16 dd '0'
align 64
shb_c    db  0,1,4,5,8,9,12,13,-1,-1,-1,-1,-1,-1,-1,-1
         db  0,1,4,5,8,9,12,13,-1,-1,-1,-1,-1,-1,-1,-1
         db  0,1,4,5,8,9,12,13,-1,-1,-1,-1,-1,-1,-1,-1
         db  0,1,4,5,8,9,12,13,-1,-1,-1,-1,-1,-1,-1,-1
align 64
idxb_c   dq 0,2,4,6,0,2,4,6
align 8
p1lo_c   dq "17181036"
p1hi_c   dw "20"
p2_c     dq "518214"

section .bss
alignb 64
buf0     resb 8192
buf1     resb 8192
alignb 64
stbuf0   resb 64
stbuf1   resb 64
ts0      resq 4
gdigits  resq 1
p1hits   resq 1
pairs    resq 1
finishd  resq 1
stopf    resq 1
sleepts  resq 2
seeds    resq 2
printb   resb 64
stacks   resb 131072

section .text
global _start

%macro GEN32 1
    vpsrld  zmm1, zmm0, 13
    vpxord  zmm0, zmm0, zmm1
    vpslld  zmm1, zmm0, 17
    vpxord  zmm0, zmm0, zmm1
    vpsrld  zmm1, zmm0, 5
    vpxord  zmm0, zmm0, zmm1
    vpmuludq zmm1, zmm0, zmm9
    vpsrlq  zmm2, zmm0, 32
    vpsrlq  zmm3, zmm9, 32
    vpmuludq zmm2, zmm2, zmm3
    vpsrlq  zmm1, zmm1, 32
    vpsrlq  zmm2, zmm2, 32
    vpsllq  zmm2, zmm2, 32
    vpblendmd zmm1{k2}, zmm1, zmm2
    vpsrld  zmm1, zmm1, 5
    vpmulld zmm1, zmm1, zmm10
    vpsubd  zmm1, zmm0, zmm1
    vpmulld zmm2, zmm1, zmm11
    vpsrld  zmm2, zmm2, 11
    vpmulld zmm3, zmm2, zmm12
    vpsubd  zmm3, zmm1, zmm3
    vpaddd  zmm2, zmm2, zmm13
    vpaddd  zmm3, zmm3, zmm13
    vpslld  zmm3, zmm3, 8
    vpord   zmm2, zmm2, zmm3
    vpshufb zmm2, zmm2, zmm14
    vpermq  zmm2, zmm15, zmm2
    vmovdqu yword [%1], ymm2
%endmacro

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
    mov [seeds], rax
    mov rcx, 0x9E3779B97F4A7C15
    xor rax, rcx
    mov [seeds + 8], rax
    mov qword [sleepts], 0
    mov qword [sleepts + 8], 100000000

    mov r12d, 0
    mov rdi, CLONE_FLAGS
    lea rsi, [stacks + 65536]
    xor edx, edx
    xor r10d, r10d
    xor r8d, r8d
    mov eax, SYS_CLONE
    syscall
    test eax, eax
    jz .child

    mov r12d, 1
    mov rdi, CLONE_FLAGS
    lea rsi, [stacks + 131072]
    xor edx, edx
    xor r10d, r10d
    xor r8d, r8d
    mov eax, SYS_CLONE
    syscall
    test eax, eax
    jz .child

.timer:
    mov edi, 1
    lea rsi, [ts0 + 16]
    mov eax, SYS_CLK
    syscall
mov rax, [ts0 + 16]
    sub rax, [ts0]
    imul rax, rax, 1000000000
    mov rdx, [ts0 + 24]
    sub rdx, [ts0 + 8]
    add rax, rdx
    mov rdx, RUNTIME
    imul rdx, rdx, 1000000000
    cmp rax, rdx
    jb .tick
    mov qword [stopf], 1
    jmp .join
.tick:
    lea rdi, [sleepts]
    xor esi, esi
    mov eax, SYS_NANOSLEEP
    syscall
    jmp .timer
.join:
    cmp qword [finishd], 2
    jae .print
    mov eax, SYS_YIELD
    syscall
    jmp .join

.child:
    call worker
    lock inc qword [finishd]
    xor edi, edi
    mov eax, SYS_EXIT
    syscall

.print:
    mov edi, 1
    lea rsi, [ts0 + 16]
    mov eax, SYS_CLK
    syscall

    mov rsi, msg_head
    mov rdx, msg_head.len
    call print

    mov rsi, msg_dig
    mov rdx, msg_dig.len
    call print
    mov rdi, [gdigits]
    call u64str
    call print

    mov rax, [ts0 + 16]
    sub rax, [ts0]
    mov r10, 1000000000
    mul r10
    mov rbx, rax
    mov rax, [ts0 + 24]
    sub rax, [ts0 + 8]
    add rbx, rax

    mov rsi, msg_time
    mov rdx, msg_time.len
    call print
    mov rax, rbx
    xor edx, edx
    mov r10, 1000000000
    div r10
    mov rdi, rax
    call u64str
    call print
    mov rsi, msg_sec
    mov rdx, msg_sec.len
    call print

    mov rsi, msg_rate
    mov rdx, msg_rate.len
    call print
    mov rax, [gdigits]
    mov r10, 1000000000
    mul r10
    div rbx
    xor edx, edx
    mov r10, 1000000
    div r10
    mov rdi, rax
    call u64str
    call print
    mov rsi, msg_mn
    mov rdx, msg_mn.len
    call print

    mov rsi, msg_p1
    mov rdx, msg_p1.len
    call print
    mov rdi, [p1hits]
    call u64str
    call print

    mov rsi, msg_pairs
    mov rdx, msg_pairs.len
    call print
    mov rdi, [pairs]
    call u64str
    call print
    mov rsi, msg_nl
    mov rdx, 1
    call print

    xor edi, edi
    mov eax, SYS_EXITGRP
    syscall

worker:
    mov rdi, r12
    shl rdi, 13
    lea rbx, [buf0]
    add rbx, rdi
    lea rdi, [stbuf0]
    mov rax, r12
    shl rax, 6
    add rdi, rax

    mov r11, [seeds + r12*8]
    xor r10d, r10d
.init_loop:
    mov rax, 0x9E3779B97F4A7C15
    add r11, rax
    mov rax, r11
    mov rcx, rax
    shr rcx, 30
    xor rax, rcx
    mov rcx, 0xBF58476D1CE4E5B9
    imul rax, rcx
    mov rcx, rax
    shr rcx, 27
    xor rax, rcx
    mov rcx, 0x94D049BB133111EB
    imul rax, rcx
    mov rcx, rax
    shr rcx, 31
    xor rax, rcx
    mov [rdi + r10*4], eax
    inc r10
    cmp r10, 16
    jb .init_loop

    vmovdqu64 zmm0, [rdi]
    vmovdqu64 zmm8, [ones_c]
    vmovdqu64 zmm9, [m100_c]
    vmovdqu64 zmm10, [c100_c]
    vmovdqu64 zmm11, [c205_c]
    vmovdqu64 zmm12, [c10_c]
    vmovdqu64 zmm13, [char0_c]
    vmovdqu64 zmm14, [shb_c]
    vmovdqu64 zmm15, [idxb_c]
    mov eax, 0xAAAA
    kmovw k2, eax
    mov r8, 0xE38E38E38E38E38F

    xor r14, r14
    mov rax, 0x3030303030303030
    mov qword [rbx], rax
    mov qword [rbx + 8], rax
    mov qword [rbx + 16], rax
    mov qword [rbx + 24], rax

.block:
    lea r9, [rbx + 32]
    mov r10d, CHUNKS
.chunk:
    GEN32 r9
    GEN32 r9+32
    mov rax, 0x9E3779B97F4A7C15
    add r11, rax
    mov rax, r11
    mov rcx, rax
    shr rcx, 30
    xor rax, rcx
    mov rcx, 0xBF58476D1CE4E5B9
    imul rax, rcx
    mov rcx, rax
    shr rcx, 27
    xor rax, rcx
    mov rcx, 0x94D049BB133111EB
    imul rax, rcx
    mov rcx, rax
    shr rcx, 31
    xor rax, rcx
mov rcx, rax
    mul r8
    shr rdx, 3
    lea rdx, [rdx + rdx*8]
    sub rcx, rdx
    add cl, '1'
    mov [r9], cl
    add r9, LEN
    dec r10d
    jnz .chunk

    add r14, NEWD

    mov r13d, 11
.scan:
    cmp r13, S - 64
    ja .scan_done
    vmovdqu64 zmm1, [rbx + r13]
    vpcmpeqb k1, zmm1, zmm8
    kmovq rax, k1
.bits:
    test rax, rax
    jz .nextblk
    bsf rcx, rax
    lea rsi, [rax - 1]
    and rax, rsi
    lea rdx, [r13 + rcx]
    cmp rdx, PMAX
    ja .scan_done
    mov rsi, [rbx + rdx]
    cmp rsi, [p1lo_c]
    jne .bits
    movzx esi, word [rbx + rdx + 8]
    cmp si, [p1hi_c]
    jne .bits
    lock inc qword [p1hits]
    xor edi, edi
.gap:
    cmp edi, 6
    jae .bits
    lea rsi, [rdx + 10]
    add rsi, rdi
    mov rsi, [rbx + rsi]
    shl rsi, 16
    shr rsi, 16
    cmp rsi, [p2_c]
    jne .gapn
    lock inc qword [pairs]
    jmp .bits
.gapn:
    inc edi
    jmp .gap
.nextblk:
    add r13, 64
    jmp .scan
.scan_done:
    cmp qword [stopf], 0
    jne .done
    vmovdqu ymm1, [rbx + NEWD]
    vmovdqu [rbx], ymm1
    jmp .block

.done:
    lock add [gdigits], r14
    ret

print:
    mov rax, SYS_WRITE
    mov rdi, 1
    syscall
    ret

u64str:
    mov rax, rdi
    mov r9, 0xCCCCCCCCCCCCCCCD
    lea r8, [printb + 31]
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