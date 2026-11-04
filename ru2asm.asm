bits 64
default rel

%define SYS_READ    0
%define SYS_WRITE   1
%define SYS_OPEN    2
%define SYS_CLOSE   3
%define SYS_EXIT    60
%define SYS_EXITGRP 231
%define SYS_CLK     228
%define SYS_CLONE   56
%define SYS_YIELD   24
%define NLEN 60
%ifndef RUNTIME
%define RUNTIME 60
%endif
%define CLONE_FLAGS 0x00050F00
%define K1  0x9E3779B97F4A7C15
%define K2  0xBF58476D1CE4E5B9
%define RNGM 0x2545F4914F6CDD1D
%define MAGIC10 0xCCCCCCCCCCCCCCCD
%define EMPTY 0xFF

section .data
dict_path  db "dict.bin", 0
msg_head   db 10, "========== ASM: 60-значные строки, слова 5+ букв, 2 потока ==========", 10
.len equ $ - msg_head
msg_dict   db "словарь: "
.len equ $ - msg_dict
msg_dict2  db " слов (5..8 букв), кодовая таблица загружена", 10
.len equ $ - msg_dict2
msg_total  db "строк прогнано: "
.len equ $ - msg_total
msg_speed1 db 10, "скорость: "
.len equ $ - msg_speed1
msg_speed2 db " строк/сек = "
.len equ $ - msg_speed2
msg_speed3 db " млн цифр/сек", 10
.len equ $ - msg_speed3
msg_c2     db "2 сл.: "
.len equ $ - msg_c2
msg_c3     db 10, "3 сл.: "
.len equ $ - msg_c3
msg_c4     db 10, "4 сл.: "
.len equ $ - msg_c4
msg_c5     db 10, "5 сл.: "
.len equ $ - msg_c5
msg_c6     db 10, "6+ сл.: "
.len equ $ - msg_c6
msg_rep    db 10, "повторов: "
.len equ $ - msg_rep
msg_gram   db 10, "без глагола/клея: "
.len equ $ - msg_gram
msg_full   db 10, "полных 60/60: "
.len equ $ - msg_full
msg_max    db 10, "макс слов: "
.len equ $ - msg_max
msg_samp   db 10, 10, "--- примеры (4+ слов, слова 5+ букв) ---", 10
.len equ $ - msg_samp
msg_arrow  db "  =>  "
.len equ $ - msg_arrow
msg_meta1  db "  ("
.len equ $ - msg_meta1
msg_meta2  db " сл., "
.len equ $ - msg_meta2
msg_meta3  db "/60 цифр)", 10
.len equ $ - msg_meta3
msg_space  db " "
.len equ $ - msg_space
msg_nodict db "нет dict.bin", 10
.len equ $ - msg_nodict

section .bss
dictbuf   resb 1048576
ts0       resq 4
tsb       resb 32
seeds     resq 2
counters  resq 8
fullcov   resq 1
seln      resb 2
lastv     resb 2
bestf     resb 2
bestc     resq 2
maxnw     resb 2
dbuf      resb 128
sela      resb 320
samplecnt resq 1
samples   resb 768
finished  resq 1
printb    resb 64
nwordsv   resq 1
stacks    resb 131072

section .text
global _start

_start:
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15

    lea rdi, [dict_path]
    xor esi, esi
    xor edx, edx
    mov eax, SYS_OPEN
    syscall
    test rax, rax
    js .nodict
    mov rdi, rax
    lea rsi, [dictbuf]
    mov edx, 1048576
    mov eax, SYS_READ
    syscall
    mov eax, SYS_CLOSE
    syscall

    lea rsi, [dictbuf]
    mov eax, [rsi + 8]
    mov [nwordsv], rax
    mov r13d, [rsi + 4]
    dec r13
    lea r15, [rsi + 32]
    mov eax, [rsi + 4]
    shl rax, 4
    lea r14, [r15 + rax]

    mov edi, 1
    lea rsi, [ts0]
    mov eax, SYS_CLK
    syscall

    mov rax, [ts0]
    xor rax, [ts0 + 8]
    jnz .sk
    mov eax, 0x9E3779B9
.sk:
    mov [seeds], rax
    mov rcx, K1
    xor rax, rcx
    mov [seeds + 8], rax

    mov r12d, 1
    mov rdi, CLONE_FLAGS
    lea rsi, [stacks + 65536]
    xor edx, edx
    xor r10d, r10d
    xor r8d, r8d
    mov eax, SYS_CLONE
    syscall
    test eax, eax
    jnz .parent

.child:
    call worker
    lock inc qword [finished]
    xor edi, edi
    mov eax, SYS_EXIT
    syscall

.parent:
    xor r12d, r12d
    call worker
    lock inc qword [finished]
.wt:
    cmp qword [finished], 2
    jae .print
    mov eax, SYS_YIELD
    syscall
    jmp .wt

.print:
    mov edi, 1
    lea rsi, [ts0 + 16]
    mov eax, SYS_CLK
    syscall

    mov rsi, msg_head
    mov rdx, msg_head.len
    call print

    mov rsi, msg_dict
    mov rdx, msg_dict.len
    call print
    mov rdi, [nwordsv]
    call u64str
    call print
    mov rsi, msg_dict2
    mov rdx, msg_dict2.len
    call print

    mov rsi, msg_total
    mov rdx, msg_total.len
    call print
    mov rdi, [counters]
    call u64str
    call print

    mov rax, [ts0 + 16]
    sub rax, [ts0]
    mov r10, 1000000000
    mul r10
    mov r11, rax
    mov rax, [ts0 + 24]
    sub rax, [ts0 + 8]
    add rax, r11
    mov rbx, rax

    mov rsi, msg_speed1
    mov rdx, msg_speed1.len
    call print
    mov rax, [counters]
    mov r10, 1000000000
    mul r10
    div rbx
    mov rdi, rax
    call u64str
    call print

    mov rsi, msg_speed2
    mov rdx, msg_speed2.len
    call print
    mov rax, [counters]
    imul rax, rax, NLEN
    imul rax, rax, 1000
    xor edx, edx
    div rbx
    mov rdi, rax
    call u64str
    call print
    mov rsi, msg_speed3
    mov rdx, msg_speed3.len
    call print

    mov rsi, msg_c2
    mov rdx, msg_c2.len
    call print
    mov rdi, [counters + 8]
    call u64str
    call print

    mov rsi, msg_c3
    mov rdx, msg_c3.len
    call print
    mov rdi, [counters + 16]
    call u64str
    call print

    mov rsi, msg_c4
    mov rdx, msg_c4.len
    call print
    mov rdi, [counters + 24]
    call u64str
    call print

    mov rsi, msg_c5
    mov rdx, msg_c5.len
    call print
    mov rdi, [counters + 32]
    call u64str
    call print

    mov rsi, msg_c6
    mov rdx, msg_c6.len
    call print
    mov rdi, [counters + 40]
    call u64str
    call print

    mov rsi, msg_rep
    mov rdx, msg_rep.len
    call print
    mov rdi, [counters + 48]
    call u64str
    call print

    mov rsi, msg_gram
    mov rdx, msg_gram.len
    call print
    mov rdi, [counters + 56]
    call u64str
    call print

    mov rsi, msg_full
    mov rdx, msg_full.len
    call print
    mov rdi, [fullcov]
    call u64str
    call print

    mov rsi, msg_max
    mov rdx, msg_max.len
    call print
    movzx edi, byte [maxnw]
    movzx eax, byte [maxnw + 1]
    cmp eax, edi
    jbe .mx
    mov edi, eax
.mx:
    call u64str
    call print

    mov rsi, msg_samp
    mov rdx, msg_samp.len
    call print

    xor ebx, ebx
.sloop:
    cmp rbx, [samplecnt]
    jae .sdone
    cmp rbx, 3
    jae .sdone
    mov rax, rbx
    shl rax, 8
    mov r8, samples
    add r8, rax
    xor r9d, r9d
.dig:
    movzx eax, byte [r8 + r9]
    mov rdi, printb
    mov [rdi], al
    mov rsi, rdi
    mov rdx, 1
    call print
    inc r9
    cmp r9, NLEN
    jb .dig
    mov rsi, msg_arrow
    mov rdx, msg_arrow.len
    call print
    movzx r10d, byte [r8 + 60]
    xor r9d, r9d
.wloop:
    cmp r9, r10
    jae .wdone
    mov rax, r9
    shl rax, 4
    mov rdx, [r8 + 64 + rax + 8]
    mov rsi, rdx
    mov rax, rdx
    mov rcx, K1
    imul rax, rcx
    mov rcx, rax
    shr rcx, 29
    xor rax, rcx
    mov rcx, K2
    imul rax, rcx
    mov rcx, rax
    shr rcx, 32
    xor rax, rcx
    and rax, r13
.p2:
    mov rcx, rax
    shl rcx, 4
    cmp byte [r15 + rcx + 8], EMPTY
    je .wnw
    cmp [r15 + rcx], rsi
    je .wfound
    inc rax
    and rax, r13
    jmp .p2
.wfound:
    movzx edx, word [r15 + rcx + 10]
    mov eax, [r15 + rcx + 12]
    lea rsi, [r14 + rax]
    call print
    mov rsi, msg_space
    mov rdx, 1
    call print
.wnw:
    inc r9
    jmp .wloop
.wdone:
    mov rsi, msg_meta1
    mov rdx, msg_meta1.len
    call print
    movzx edi, byte [r8 + 60]
    call u64str
    call print
    mov rsi, msg_meta2
    mov rdx, msg_meta2.len
    call print
    mov rax, rbx
    shl rax, 8
    mov r8, samples
    add r8, rax
    movzx edi, byte [r8 + 61]
    call u64str
    call print
    mov rsi, msg_meta3
    mov rdx, msg_meta3.len
    call print
    inc rbx
    jmp .sloop
.sdone:

    xor edi, edi
    mov eax, SYS_EXITGRP
    syscall

.nodict:
    mov rsi, msg_nodict
    mov rdx, msg_nodict.len
    call print
    xor edi, edi
    mov eax, SYS_EXIT
    syscall

worker:
    mov r10, [seeds + r12*8]
    mov rbx, r12
    shl rbx, 6
    mov rax, dbuf
    add rbx, rax
    mov rbp, r12
    imul rbp, rbp, 160
    mov rax, sela
    add rbp, rax
    mov byte [seln + r12], 0
    mov byte [lastv + r12], 0xFF
    mov byte [bestf + r12], 0
    mov qword [bestc + r12*8], 0
    mov byte [maxnw + r12], 0
    xor r11d, r11d

.loop:
    inc r11
    test r11, 0xFFF
    jnz .nogogo
    lock add qword [counters], 4096
    mov edi, 1
    mov rax, r12
    shl rax, 4
    lea rsi, [tsb]
    add rsi, rax
    mov eax, SYS_CLK
    syscall
    mov rax, [rsi]
    sub rax, [ts0]
    cmp rax, RUNTIME
    jb .nogogo
    jmp .done
.nogogo:
    xor esi, esi
.gen:
    mov rax, r10
    mov rcx, rax
    shr rcx, 12
    xor rax, rcx
    mov rcx, rax
    shl rcx, 25
    xor rax, rcx
    mov rcx, rax
    shr rcx, 27
    xor rax, rcx
    mov r10, rax
    mov rdx, RNGM
    imul rax, rdx
    mov rcx, rax
    mov rdx, MAGIC10
    mul rdx
    shr rdx, 3
    lea r8, [rdx + rdx*4]
    add r8, r8
    sub rcx, r8
    test rsi, rsi
    jnz .put
    test rcx, rcx
    jnz .put
    mov ecx, 1
.put:
    add cl, '0'
    mov [rbx + rsi], cl
    inc rsi
    cmp rsi, NLEN
    jb .gen

    mov byte [seln + r12], 0
    mov byte [lastv + r12], 0xFF
    xor edi, edi
.scan_i:
    xor r8d, r8d
    xor r9d, r9d
    mov byte [bestf + r12], 0
    mov ecx, 1
    mov rsi, rdi
.scan_ln:
    movzx rax, byte [rbx + rsi]
    sub rax, '0'
    lea r8, [r8 + r8*4]
    add r8, r8
    add r8, rax
    inc rsi
    cmp rcx, 5
    jb .ln_next
    mov rax, r8
    mov rdx, K1
    imul rax, rdx
    mov rdx, rax
    shr rdx, 29
    xor rax, rdx
    mov rdx, K2
    imul rax, rdx
    mov rdx, rax
    shr rdx, 32
    xor rax, rdx
    and rax, r13
.ph:
    mov rdx, rax
    shl rdx, 4
    cmp byte [r15 + rdx + 8], EMPTY
    je .ln_next
    cmp [r15 + rdx], r8
    je .hit
    inc rax
    and rax, r13
    jmp .ph
.hit:
    mov r9, rsi
    movzx eax, byte [r15 + rdx + 8]
    mov [bestf + r12], al
    mov [bestc + r12*8], r8
.ln_next:
    inc rcx
    cmp rcx, 16
    ja .i_done
    cmp rsi, NLEN
    jae .i_done
    jmp .scan_ln
.i_done:
    test r9, r9
    jz .next_i
    movzx eax, byte [lastv + r12]
    movsx eax, al
    cmp edi, eax
    jle .next_i
    movzx eax, byte [seln + r12]
    cmp eax, 10
    jae .next_i
    mov rdx, rax
    shl rdx, 4
    add rdx, rbp
    mov byte [rdx], dil
    mov byte [rdx + 1], r9b
    mov al, [bestf + r12]
    mov byte [rdx + 2], al
    mov rax, [bestc + r12*8]
    mov [rdx + 8], rax
    mov al, [seln + r12]
    inc al
    mov [seln + r12], al
    mov [lastv + r12], r9b
.next_i:
    inc rdi
    cmp rdi, NLEN
    jb .scan_i

    movzx ecx, byte [seln + r12]
    cmp ecx, 2
    jb .next_string

    xor esi, esi
.dup1:
    cmp esi, ecx
    jae .nodup
    mov rax, rsi
    shl rax, 4
    mov r8, [rbp + rax + 8]
    lea edi, [esi + 1]
.dup2:
    cmp edi, ecx
    jae .dup1n
    mov rax, rdi
    shl rax, 4
    cmp r8, [rbp + rax + 8]
    je .rep
    inc edi
    jmp .dup2
.dup1n:
    inc esi
    jmp .dup1
.rep:
    lock inc qword [counters + 48]
    jmp .next_string
.nodup:
    xor r8d, r8d
    xor r9d, r9d
    xor esi, esi
.cat:
    cmp esi, ecx
    jae .catdone
    mov rax, rsi
    shl rax, 4
    movzx edx, byte [rbp + rax + 2]
    cmp edx, 1
    jne .c1
    inc r8d
    jmp .catn
.c1:
    cmp edx, 2
    jne .c2
    inc r9d
    jmp .catn
.c2:
    cmp edx, 3
    je .gram
.catn:
    inc esi
    jmp .cat
.catdone:
    test r8d, r8d
    jz .gram
    cmp r9d, 2
    ja .gram
    cmp ecx, 2
    jne .c3
    lock inc qword [counters + 8]
    jmp .stats
.c3:
    cmp ecx, 3
    jne .c4
    lock inc qword [counters + 16]
    jmp .stats
.c4:
    cmp ecx, 4
    jne .c5
    lock inc qword [counters + 24]
    jmp .stats
.c5:
    cmp ecx, 5
    jne .c6
    lock inc qword [counters + 32]
    jmp .stats
.c6:
    lock inc qword [counters + 40]
.stats:
    movzx eax, byte [maxnw + r12]
    cmp ecx, eax
    jbe .fullck
    mov [maxnw + r12], cl
.fullck:
    mov eax, ecx
    dec eax
    mov rdx, rax
    shl rdx, 4
    movzx eax, byte [rbp + rdx + 1]
    movzx edx, byte [rbp]
    sub eax, edx
    cmp eax, NLEN
    jne .sample
    lock inc qword [fullcov]
.sample:
    cmp ecx, 2
    jb .next_string
    mov rax, 1
    lock xadd qword [samplecnt], rax
    cmp rax, 3
    jae .next_string
    shl rax, 8
    mov rsi, samples
    add rsi, rax
    xor edi, edi
.cp:
    mov al, [rbx + rdi]
    mov [rsi + rdi], al
    inc edi
    cmp edi, NLEN
    jb .cp
    mov [rsi + 60], cl
    mov eax, ecx
    dec eax
    shl rax, 4
    movzx edx, byte [rbp + rax + 1]
    movzx eax, byte [rbp]
    sub edx, eax
    mov [rsi + 61], dl
    xor edi, edi
.cs:
    cmp edi, ecx
    jae .next_string
    mov rax, rdi
    shl rax, 4
    mov rdx, [rbp + rax]
    mov [rsi + 64 + rax], rdx
    mov rdx, [rbp + rax + 8]
    mov [rsi + 64 + rax + 8], rdx
    inc edi
    jmp .cs
.gram:
    lock inc qword [counters + 56]
    jmp .next_string
.next_string:
    jmp .loop
.done:
    mov rax, r11
    and rax, 0xFFF
    jz .ret
    lock add qword [counters], rax
.ret:
    ret

print:
    mov rax, SYS_WRITE
    mov rdi, 1
    syscall
    ret

u64str:
    mov rax, rdi
    mov r9, MAGIC10
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